//! `ff commit` — the close. The working tree IS the open change; closing it
//! builds the `add -A` tree, writes an ordinary commit with the USER's
//! identity, advances the branch, and rewrites the index to match — the
//! next edit opens the next change. A clean tree closes nothing, whatever
//! the message — fufu writes no empty commit.
//!
//! Ordering is write-ahead: reconcile → capture → hooks → tree + message +
//! plan → append the operation → mutate (branch axis, ref CAS, index, pending
//! description). A crash after the append is labeled loudly by the next
//! reconcile.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::hooks;
use crate::model::{CommitOutcome, HeadState};
use crate::ops::record::observe_refs;
use crate::ops::{DescriptionTransition, OpKind, OpRecord, RefTransition, verb};
use crate::refs;
use crate::snapshot::tree as snaptree;
use crate::snapshot::{Provenance, config};

#[derive(Debug, Clone, Default)]
pub struct CloseOptions {
    /// `-m`: describes what is closing; wins over the pending description.
    pub message: Option<String>,
    /// Skip pre-commit and commit-msg hooks.
    pub no_verify: bool,
    /// `-b`: placeholder branch → claim-rename; fresh name → the close
    /// lands on a new branch forked here, the old branch stays.
    pub branch: Option<String>,
    /// `<paths>`: close only what lies under these, leaving the rest open.
    /// The rule is [`crate::restore::path_selected`]'s — a file path or a
    /// directory prefix, no globs. Empty closes the whole open change.
    pub paths: Vec<String>,
    /// Clock injection for tests.
    pub now: Option<i64>,
    /// The invoking argv, recorded verbatim.
    pub argv: Vec<String>,
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// The paths a scan saw that a slice does not take — what a close leaves on
/// disk. Empty paths select everything, so nothing is left behind.
fn unselected_paths(scan: &snaptree::Scan, paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return Vec::new();
    }
    scan.paths()
        .filter(|path| !crate::restore::path_selected(path, paths))
        .map(String::from)
        .collect()
}

/// The clean-tree refusal: no hooks, no commit, nothing written. It happens
/// before every mutation, so a pending description survives it untouched; a
/// `-m` does not — it is discarded with the refusal, which is why
/// `ff describe -m` is one of the exits.
fn empty_refusal(branch: &str, pending: Option<&str>, paths: &[String]) -> Error {
    let (mut message, exits) = if paths.is_empty() {
        (
            format!(
                "nothing to close on {branch}: the tree matches HEAD, and fufu writes no empty commit"
            ),
            vec!["ff status".into(), "ff describe -m <message>".into()],
        )
    } else {
        (
            format!(
                "nothing to close under {} on {branch}: those paths match HEAD, and fufu writes no \
                 empty commit",
                paths.join(", ")
            ),
            vec!["ff diff <path>".into(), "ff status".into()],
        )
    };
    if pending.is_some() {
        message.push_str("; the pending description stays put");
    }
    Error::coded("commit/empty", message, exits)
}

/// A path that names nothing on disk or in HEAD: refused, not committed with a
/// hole in it. A sentence in the path slot is almost always a forgotten `-m`,
/// so the exits then lead with the flag-shaped one.
fn no_such_path(token: &str) -> Error {
    let exits = if token.chars().any(char::is_whitespace) {
        vec![format!("ff commit -m {token:?}"), "ff status".into()]
    } else {
        vec!["ff status".into(), "ff commit".into()]
    };
    Error::coded(
        "usage/no-such-path",
        format!("no path here goes by {token:?}: `ff commit` takes file and directory paths"),
        exits,
    )
}

/// Close the open change. `prov` names the mandatory pre-verb capture.
pub fn close(
    repo: &gix::Repository,
    opts: &CloseOptions,
    prov: &Provenance,
) -> Result<(CommitOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to commit",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git commit / git merge --abort); \
                 fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }

    let head = crate::head::head_state(repo)?;

    // The session guard sits ahead of the capture floor: refusing before the
    // capture means nothing at all is written to learn that a session is
    // running. A session branch's whole content is the amendment of the
    // commit under its feet, and a commit landed on that branch puts fufu in
    // a state no other verb can describe. `ff commit` inside a session is
    // `ff done` under another name, which is what the refusal says.
    if let HeadState::Branch { name, commit, .. } = &head
        && branchmeta::read(repo, name)?.session.is_some()
    {
        let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
        let short = crate::sha::short_oid(tip);
        let subject = subject(repo, tip)?;
        return Err(Error::coded(
            "session/open",
            format!(
                "{name} is an editing session on {short} \"{subject}\": a commit here would land \
                 somewhere no verb can describe"
            ),
            vec![
                "ff done".into(),
                "ff done --abandon".into(),
                "ff switch <branch>".into(),
            ],
        ));
    }

    // A path that names nothing is a typo or a forgotten -m, not a commit
    // with a hole in it. Refuse before the capture floor so nothing at all
    // is written to learn it; the bare/mid-operation/session refusals above
    // still lead.
    for path in &opts.paths {
        if !crate::restore::path_exists(repo, path)? {
            return Err(no_such_path(path));
        }
    }

    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;

    let (current_branch, head_commit) = match &head {
        HeadState::Branch { name, commit, .. } => (
            name.clone(),
            Some(gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?),
        ),
        HeadState::Unborn { r#ref } => (
            r#ref
                .strip_prefix("refs/heads/")
                .unwrap_or(r#ref)
                .to_string(),
            None,
        ),
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: there is no branch to close onto",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    // Read branchmeta early so emptiness can consult the pending description.
    let meta = branchmeta::read(repo, &current_branch)?;
    let pending = meta.pending_description.clone();

    // Emptiness first (git's order too): a clean slice runs no hooks and
    // closes nothing, whatever the message. Emptiness is judged on the
    // narrowed scan — a clean slice refuses the way a clean tree does.
    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let mut scan_full = snaptree::scan(repo)?;
    let mut scan_slice = snaptree::scan(repo)?.narrowed(&opts.paths);
    if scan_slice.is_empty() {
        return Err(empty_refusal(
            &current_branch,
            pending.as_deref(),
            &opts.paths,
        ));
    }

    // Hook-runners — lefthook, lint-staged, husky, pre-commit — ask git
    // what is staged and do nothing when the answer is empty. fufu's index
    // describes the last commit while a change is open, so every one of
    // them silently no-ops. Git populates the index before running
    // `pre-commit` (both `commit -a` and `commit -- <path>` do) and rolls it
    // back when the commit does not land; do the same. The index stays a
    // derived surface the user never maintains — it is just written at the
    // right moment now.
    //
    // The provisional tree is the *slice*, so a partial `ff commit <paths>`
    // stages exactly what is landing, as git's pathspec form does.
    let mut index_backup = None;
    if !opts.no_verify && hooks::will_run(repo)? {
        // Never `head_tree`: `snaptree::scan` short-circuits on a valid
        // cache-tree root equal to HEAD's tree and would then report only
        // index↔worktree, so a provisional index equal to HEAD (or written
        // without a cache tree) would make the re-scan below come back
        // empty and refuse a real change as `commit/empty`. The empty
        // slice already refused above, so this tree differs from HEAD.
        let provisional = snaptree::assemble(repo, head_tree, &scan_slice, u64::MAX)?.0;
        let differs = unselected_paths(&scan_full, &opts.paths);
        index_backup = Some(crate::index::IndexBackup::take(repo)?);
        crate::index::write_index_for_tree_except(repo, provisional, &differs)?;
    }

    // Hooks before the tree build — pre-commit hooks format files, so a
    // hook that ran invalidates the scan. Re-take both, and re-narrow.
    if !opts.no_verify && hooks::pre_commit(repo)? {
        scan_full = snaptree::scan(repo)?;
        scan_slice = snaptree::scan(repo)?.narrowed(&opts.paths);
        if scan_slice.is_empty() {
            return Err(empty_refusal(
                &current_branch,
                pending.as_deref(),
                &opts.paths,
            ));
        }
    }

    // The close tree is exact: nothing is size-capped out of a commit. An
    // empty slice already refused above, so a close always assembles a
    // real tree.
    let (commit_tree, _skipped) = snaptree::assemble(repo, head_tree, &scan_slice, u64::MAX)?;
    if commit_tree == head_tree {
        return Err(empty_refusal(
            &current_branch,
            pending.as_deref(),
            &opts.paths,
        ));
    }
    // The working tree keeps what the close did not take. With no paths the
    // two are the one tree, as before; a second identical assembly on that
    // common path is pure cost, so only a real slice assembles the
    // remainder.
    let worktree_tree = if opts.paths.is_empty() {
        commit_tree
    } else {
        snaptree::assemble(repo, head_tree, &scan_full, u64::MAX)?.0
    };

    // What the close is leaving on disk: everything the scan saw that the
    // slice did not take. The index is about to be written to `commit_tree`,
    // which for these paths is HEAD's blob rather than what the worktree
    // holds — so their stat data must not be carried over, or the next
    // status trusts it and the remainder stops being the open change. See
    // `index::write_index_for_tree_except`.
    let worktree_differs = unselected_paths(&scan_full, &opts.paths);

    // Message: -m beats the pending description; either way the pending
    // description is consumed by the close.
    let mut message = opts
        .message
        .clone()
        .or_else(|| pending.clone())
        .unwrap_or_default();
    if !opts.no_verify {
        message = hooks::commit_msg(repo, &message)?;
    }
    let message = normalize_message(&message);
    let subject = message
        .lines()
        .next()
        .unwrap_or("(no description)")
        .to_string();

    // The branch axis: where does this close land?
    let mut claim_from: Option<String> = None;
    let mut created_branch = false;
    let target_branch = match &opts.branch {
        None => current_branch.clone(),
        Some(name) => {
            branch::validate_name(name)?;
            if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                return Err(Error::coded(
                    "branch/exists",
                    format!("a branch named {name} already exists"),
                    vec!["ff branch".into()],
                ));
            }
            if branch::is_anonymous(&current_branch) {
                claim_from = Some(current_branch.clone());
            } else {
                created_branch = true;
            }
            name.clone()
        }
    };

    // The commit object, written up front — the plan needs its sha.
    let sig = refs::user_signature(repo, now)?;
    let parents: Vec<gix::ObjectId> = head_commit.into_iter().collect();
    let commit = gix::objs::Commit {
        tree: commit_tree,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig.clone(),
        encoding: None,
        message: message.clone().into(),
        extra_headers: Vec::new(),
    };
    let commit_id = repo.write_object(&commit).map_err(Error::repo)?.detach();

    // Write-ahead: the planned table is the post-close world.
    let target_ref = format!("refs/heads/{target_branch}");
    let mut planned = observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    if let Some(old_name) = &claim_from {
        let old_ref = format!("refs/heads/{old_name}");
        planned.refs.remove(&old_ref);
        transitions.push(RefTransition {
            name: old_ref,
            old: head_commit.map(|c| c.to_string()),
            new: None,
        });
        if let Some(parked) = crate::stash::parked_entry(repo, old_name)? {
            let old_parked = crate::stash::parked_ref(old_name);
            planned.refs.remove(&old_parked);
            planned
                .refs
                .insert(crate::stash::parked_ref(&target_branch), parked.to_string());
        }
    }
    transitions.push(RefTransition {
        name: target_ref.clone(),
        old: match (&opts.branch, head_commit) {
            (None, Some(c)) => Some(c.to_string()),
            _ => None,
        },
        new: Some(commit_id.to_string()),
    });
    planned
        .refs
        .insert(target_ref.clone(), commit_id.to_string());
    let head_transition = (opts.branch.is_some()).then(|| {
        (
            format!("ref:refs/heads/{current_branch}"),
            format!("ref:{target_ref}"),
        )
    });
    if head_transition.is_some() {
        planned.head = format!("ref:{target_ref}");
    }

    let mut record = OpRecord::new(
        "commit",
        format!("commit on {target_branch}: {subject}"),
        now,
    );
    record.argv = opts.argv.clone();
    record.refs = transitions;
    record.head = head_transition;
    record.description = pending.as_ref().map(|text| DescriptionTransition {
        branch: current_branch.clone(),
        old: Some(text.clone()),
        new: None,
    });
    let mut pins = vec![commit_id];
    pins.extend(head_commit);
    pins.extend(ctx.pre_op.map(|id| id.object_id()));
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // No paths: all three roles hold the one tree, as before.
            // Paths: `tree` is the working directory the close leaves behind,
            // still holding the unselected edits, while `index_tree` is the
            // commit the index is rewritten to — so the remainder survives as
            // the open change. Swapping the two would make `ff undo` restore
            // a tree that never existed. And it closes a loop: HEAD carries
            // the slice, the next capture records the working tree, so
            // `change_stat` — HEAD against the newest operation's tree — is
            // exactly the unselected changes. The open change survives the
            // close, minus the slice, with no new machinery.
            tree: worktree_tree,
            index_tree: commit_tree,
            // A claim renames the branch, and the rename carries its pointer
            // into the log over to the new name; recording under the new name
            // would create that pointer first and collide. A fresh `-b` name
            // forks instead of renaming, so it opens its own pointer here.
            branch: match &claim_from {
                Some(old) => old.clone(),
                None => target_branch.clone(),
            },
            base: head_commit,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Mutate. Branch axis first, then the CAS advance, then the index.
    if let Some(old_name) = &claim_from {
        branch::rename(repo, old_name, &target_branch, now)?;
    } else if created_branch {
        if let Some(at) = head_commit {
            branch::create_at(
                repo,
                &target_branch,
                at,
                now,
                &format!("branch: forked from {current_branch}"),
            )?;
        }
        branch::retarget_head(repo, &target_ref, now)?;
    }

    let expected = match (claim_from.is_some() || created_branch, head_commit) {
        // Same branch, born: CAS against the exact tip the plan saw.
        (false, Some(c)) => {
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(c))
        }
        // A fresh -b branch was just created at HEAD: advance from there.
        (true, Some(c)) => {
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(c))
        }
        // Unborn: the close is the first commit.
        (_, None) => gix::refs::transaction::PreviousValue::MustNotExist,
    };
    let reflog_msg = match head_commit {
        Some(_) => format!("commit: {subject}"),
        None => format!("commit (initial): {subject}"),
    };
    let edit = refs::update_edit(&target_ref, commit_id, expected, &reflog_msg)?;
    let time_str = format!("{now} +0000");
    let sig_ref = gix::actor::SignatureRef {
        name: sig.name.as_ref(),
        email: sig.email.as_ref(),
        time: &time_str,
    };
    match refs::commit_edits_as(repo, Some(edit), sig_ref)? {
        refs::EditOutcome::Applied => {
            // The close has landed: the provisional index is no longer
            // provisional, and putting the old one back would contradict
            // HEAD. Every exit before this point — the post-hook empty
            // refusal, a declining `commit-msg`, `branch/exists`,
            // `ref/contended`, any `?` on the way — drops the guard armed
            // and gets the index back byte-for-byte.
            if let Some(backup) = index_backup.take() {
                backup.disarm();
            }
        }
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                format!(
                    "{target_ref} moved while closing; nothing was committed (re-run to close on the new tip)"
                ),
                vec![],
            ));
        }
    }

    // The index becomes the commit: nothing staged, next edit opens the next
    // change. The remainder keeps zeroed stats so it stays visible as the
    // open change rather than being trusted clean.
    crate::index::write_index_for_tree_except(repo, commit_tree, &worktree_differs)?;

    // Consume the pending description.
    if pending.is_some() {
        let mut meta = branchmeta::read(repo, &target_branch)?;
        meta.pending_description = None;
        branchmeta::write(repo, &target_branch, &meta)?;
        // The claim may have carried it under the old name too.
        if claim_from.is_some() {
            let mut old_meta = branchmeta::read(repo, &current_branch)?;
            old_meta.pending_description = None;
            branchmeta::write(repo, &current_branch, &old_meta)?;
        }
    }

    // First close on a logless repo: make sure gc guards exist (the log pins
    // history through refs/fufu/*).
    let _ = config::ensure_gc_config(repo);

    let files_changed = crate::snapshot::count_file_changes(repo, head_tree, commit_tree)?;
    let short_id = crate::sha::short_oid(commit_id);
    Ok((
        CommitOutcome::Closed {
            id: commit_id.to_string(),
            short_id,
            branch: target_branch,
            subject,
            files_changed,
            claimed_from: claim_from,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// Git-style minimal cleanup: strip trailing whitespace per line end, cap
/// to one trailing newline, empty stays empty.
pub(crate) fn normalize_message(message: &str) -> String {
    let trimmed = message.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    format!("{trimmed}\n")
}
