//! `ff commit` — the close. The working tree IS the open change; closing it
//! builds the `add -A` tree, moves the branch onto the open commit — the
//! commit the captures have been writing under `refs/fufu/open/<branch>`,
//! the sha the `@` row showed — and rewrites the index to match; the next
//! edit opens the next change. When the open commit cannot be what lands
//! (signing, a partial close, a hook that changed the tree or the message)
//! the close mints an ordinary commit with the USER's identity and says why.
//! A clean tree closes nothing, whatever the message — fufu writes no empty
//! commit.
//!
//! Ordering is write-ahead: reconcile → capture → hooks → tree + message +
//! plan → append the operation → mutate (branch axis, ref CAS, index, pending
//! description). A crash after the append is labeled loudly by the next
//! reconcile.

use crate::branch;
use crate::branchmeta;
use crate::changeid;
use crate::error::{Error, Result};
use crate::hooks;
use crate::model::{CommitOutcome, HeadState, Remint};
use crate::open;
use crate::ops::record::observe_refs;
use crate::ops::{
    ChangeIdTransition, DescriptionTransition, OpKind, OpRecord, RefTransition, verb,
};
use crate::refs;
use crate::sign;
use crate::snapshot::tree as snaptree;
use crate::snapshot::{Provenance, config};

#[derive(Debug, Clone, Default)]
pub struct CloseOptions {
    /// `-m`: describes what is closing; wins over the pending description.
    pub message: Option<String>,
    /// `--no-verify`: skip the pre-commit and commit-msg hooks.
    pub verify: hooks::Verify,
    /// `-b`: placeholder branch → claim-rename; fresh name → the close
    /// lands on a new branch forked here, the old branch stays.
    pub branch: Option<String>,
    /// `<paths>`: close only what lies under these, leaving the rest open.
    /// The rule is [`crate::restore::path_selected`]'s — a file path or a
    /// directory prefix, no globs. Empty closes the whole open change.
    pub paths: Vec<String>,
    /// `-S` / `--no-sign`: whether this close signs, when the answer is not
    /// simply `commit.gpgsign`'s.
    pub sign: sign::Choice,
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
    let head = refuse_before_capture(repo, opts)?;

    // The signer is resolved here, above every write and above the tree
    // work: a bad `gpg.format` or a missing key costs a config read and
    // nothing else. The spawn itself happens once, at the commit object.
    let signer = sign::resolve(repo, opts.sign)?;

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
    // The commit's identity: what a capture or a describe minted for the open
    // change, or, when neither ran, minted here as the last resort.
    let change_id = match &meta.change_id {
        Some(letters) => changeid::ChangeId::parse(letters).ok_or_else(|| {
            Error::msg(format!(
                "corrupt branch metadata for {current_branch}: change id {letters:?} is not one"
            ))
        })?,
        None => changeid::ChangeId::mint()?,
    };

    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let Trees {
        commit_tree,
        worktree_tree,
        worktree_differs,
        mut window,
    } = close_trees(repo, opts, &current_branch, pending.as_deref(), head_tree)?;

    // Message: -m beats the pending description; either way the pending
    // description is consumed by the close.
    let supplied = opts.message.clone().or_else(|| pending.clone());
    // A close never brings an editor up, so the only source it can name is
    // the text the user already supplied; with none, git names no source at
    // all and passes the file path alone.
    let source = match &supplied {
        Some(_) => hooks::MsgSource::Message,
        None => hooks::MsgSource::Unspecified,
    };
    let message = hooks::message_hooks(
        repo,
        supplied.as_deref().unwrap_or_default(),
        source,
        opts.verify,
        "commit",
    )?;
    let message = normalize_message(&message);
    let hook_changed_message =
        message != normalize_message(supplied.as_deref().unwrap_or_default());
    let subject = message
        .lines()
        .next()
        .unwrap_or("(no description)")
        .to_string();

    let Axis {
        target_branch,
        claim_from,
        created_branch,
    } = landing_branch(repo, opts, &current_branch)?;

    // The commit that lands: the open commit, when it is exactly what this
    // close would write — the pre-verb capture wrote it over this tree with
    // this message and this id, and the branch just moves onto it. Anything
    // that makes the landing commit differ is named, and the close mints one
    // of its own: the object is written up front because the plan needs its
    // sha.
    let sig = refs::user_signature(repo, now)?;
    let (commit_id, reminted) = landing_commit(
        repo,
        &ctx,
        LandingCommit {
            current_branch: &current_branch,
            head_commit,
            change_id,
            change_born: meta.change_born,
            signer: signer.as_ref(),
            partial: !opts.paths.is_empty(),
            commit_tree,
            message: &message,
            hook_changed_message,
            sig: &sig,
        },
    )?;

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
    // The id leaves the open change for the commit. A partial close leaves
    // a remainder on disk, and that remainder is a change with an identity
    // of its own from this moment, before any capture sees it.
    let remainder_id = (!worktree_differs.is_empty())
        .then(changeid::ChangeId::mint)
        .transpose()?
        .map(|id| id.letters());
    // Journaled on the branch the id was read from. Under `-b` the remainder
    // lands on the new branch instead, and that mint is not journaled, the
    // way a capture's is not: a redo leaves the remainder to mint afresh.
    let remainder_born = remainder_id.as_ref().map(|_| now);
    record.change_id = Some(ChangeIdTransition {
        branch: current_branch.clone(),
        old: meta.change_id.clone(),
        new: (current_branch == target_branch)
            .then(|| remainder_id.clone())
            .flatten(),
        old_born: meta.change_born,
        new_born: (current_branch == target_branch)
            .then_some(remainder_born)
            .flatten(),
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

    // Mutate. Branch axis first, then the CAS advance, then the index. A
    // close that leaves its branch — a claim or a fork — leaves nothing
    // open on it: the open ref goes before the rename would carry it.
    if current_branch != target_branch {
        open::clear(repo, &current_branch, now)?;
    }
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

    advance_branch(
        repo,
        &target_ref,
        commit_id,
        head_commit,
        &subject,
        &sig,
        now,
    )?;
    // The close has landed: the provisional index is no longer provisional,
    // and putting the old one back would contradict HEAD. Every exit before
    // this point — the post-hook empty refusal, a declining `commit-msg`,
    // `branch/exists`, `ref/contended`, any `?` on the way — drops the guard
    // armed and gets the index back byte-for-byte.
    if let Some(window) = window.take() {
        window.landed();
    }

    // The index becomes the commit: nothing staged, next edit opens the next
    // change. The remainder keeps zeroed stats so it stays visible as the
    // open change rather than being trusted clean.
    crate::index::write_index_for_tree_except(repo, commit_tree, &worktree_differs)?;

    // Consume the pending description and the id: the commit carries both
    // now. The remainder of a partial close is the open change of whichever
    // branch HEAD is on after the close, and its id goes there; the branch
    // the close read from is cleared when it is a different one, whether
    // the close claimed it under a new name or forked a fresh `-b` branch
    // and left it behind.
    let mut target_meta = branchmeta::read(repo, &target_branch)?;
    target_meta.pending_description = None;
    target_meta.change_id = remainder_id.clone();
    target_meta.change_born = remainder_born;
    branchmeta::write(repo, &target_branch, &target_meta)?;
    if current_branch != target_branch {
        let mut old_meta = branchmeta::read(repo, &current_branch)?;
        old_meta.pending_description = None;
        old_meta.change_id = None;
        old_meta.change_born = None;
        branchmeta::write(repo, &current_branch, &old_meta)?;
    }

    // First close on a logless repo: make sure gc guards exist (the log pins
    // history through refs/fufu/*).
    let _ = config::ensure_gc_config(repo);

    // `post-commit` last, so the hook sees the landed state: the branch is
    // where the close put it, the index matches, and the pending description
    // is gone. It is notification only — nothing it does can take the commit
    // back, which is why nothing here reads its result.
    hooks::post_commit(repo);

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
            reminted,
        },
        ctx,
    ))
}

/// The refusals that sit ahead of the capture floor: refusing before the
/// capture means nothing at all is written to learn that the close cannot
/// run. Returns HEAD as read.
fn refuse_before_capture(repo: &gix::Repository, opts: &CloseOptions) -> Result<HeadState> {
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

    Ok(head)
}

/// The trees a close writes, and the hook window it holds open.
struct Trees {
    /// The commit's tree: the slice over HEAD, exact.
    commit_tree: gix::ObjectId,
    /// What the working tree keeps: the whole scan, which is `commit_tree`
    /// when no paths narrowed the close.
    worktree_tree: gix::ObjectId,
    /// The paths the scan saw and the slice did not take. The index is
    /// about to be written to `commit_tree`, which for these paths is HEAD's
    /// blob rather than what the worktree holds — so their stat data must
    /// not be carried over, or the next status trusts it and the remainder
    /// stops being the open change. See `index::write_index_for_tree_except`.
    worktree_differs: Vec<String>,
    /// The pre-commit gate's index window, when a hook will run; disarmed by
    /// the caller once the close has landed.
    window: Option<hooks::Window>,
}

/// Scan, refuse an empty slice, run the pre-commit gate over the
/// provisional index, and assemble the trees. Emptiness first (git's order
/// too): a clean slice runs no hooks and closes nothing, whatever the
/// message, and it is judged on the narrowed scan — a clean slice refuses
/// the way a clean tree does.
fn close_trees(
    repo: &gix::Repository,
    opts: &CloseOptions,
    current_branch: &str,
    pending: Option<&str>,
    head_tree: gix::ObjectId,
) -> Result<Trees> {
    let mut scan_full = snaptree::scan(repo)?;
    let mut scan_slice = snaptree::scan(repo)?.narrowed(&opts.paths);
    if scan_slice.is_empty() {
        return Err(empty_refusal(current_branch, pending, &opts.paths));
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
    //
    // `prepare-commit-msg` is in the gate as well: it runs even under
    // `--no-verify`, and it wants the same staged index, so the window that
    // holds it open is the same one.
    let close_hooks: &[&str] = match opts.verify {
        hooks::Verify::Run => &["pre-commit", "prepare-commit-msg", "commit-msg"],
        hooks::Verify::Skip => &["prepare-commit-msg"],
    };
    let mut window = None;
    let mut hook_ran = false;
    if hooks::will_run(repo, close_hooks)? {
        // Never `head_tree`: `snaptree::scan` short-circuits on a valid
        // cache-tree root equal to HEAD's tree and would then report only
        // index↔worktree, so a provisional index equal to HEAD (or written
        // without a cache tree) would make the re-scan below come back
        // empty and refuse a real change as `commit/empty`. The empty
        // slice already refused above, so this tree differs from HEAD.
        let provisional = snaptree::assemble(repo, head_tree, &scan_slice, u64::MAX)?.0;
        let differs = snaptree::unselected_paths(&scan_full, &opts.paths);
        let (opened, ran) =
            hooks::Window::open(repo, provisional, &differs, opts.verify, "commit")?;
        window = Some(opened);
        hook_ran = ran;
    }

    // Hooks before the tree build — pre-commit hooks format files, so a
    // hook that ran invalidates the scan. Re-take both, and re-narrow.
    if hook_ran {
        scan_full = snaptree::scan(repo)?;
        scan_slice = snaptree::scan(repo)?.narrowed(&opts.paths);
        if scan_slice.is_empty() {
            return Err(empty_refusal(current_branch, pending, &opts.paths));
        }
    }

    // The close tree is exact: nothing is size-capped out of a commit. An
    // empty slice already refused above, so a close always assembles a
    // real tree.
    let (commit_tree, _skipped) = snaptree::assemble(repo, head_tree, &scan_slice, u64::MAX)?;
    if commit_tree == head_tree {
        return Err(empty_refusal(current_branch, pending, &opts.paths));
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
    // slice did not take.
    let worktree_differs = snaptree::unselected_paths(&scan_full, &opts.paths);

    Ok(Trees {
        commit_tree,
        worktree_tree,
        worktree_differs,
        window,
    })
}

/// The branch axis: where the close lands.
struct Axis {
    target_branch: String,
    /// The anonymous branch a `-b` claims under the new name.
    claim_from: Option<String>,
    /// `-b` with a fresh name on a named branch: the close forks.
    created_branch: bool,
}

/// Where this close lands: the branch underfoot, or the `-b` name — a claim
/// when the branch underfoot is anonymous, a fork otherwise.
fn landing_branch(
    repo: &gix::Repository,
    opts: &CloseOptions,
    current_branch: &str,
) -> Result<Axis> {
    let mut claim_from: Option<String> = None;
    let mut created_branch = false;
    let target_branch = match &opts.branch {
        None => current_branch.to_string(),
        Some(name) => {
            branch::validate_name(name)?;
            if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                return Err(Error::coded(
                    "branch/exists",
                    format!("a branch named {name} already exists"),
                    vec!["ff branch".into()],
                ));
            }
            if branch::is_anonymous(current_branch) {
                claim_from = Some(current_branch.to_string());
            } else {
                created_branch = true;
            }
            name.clone()
        }
    };

    Ok(Axis {
        target_branch,
        claim_from,
        created_branch,
    })
}

/// What decides the commit that lands.
struct LandingCommit<'a> {
    current_branch: &'a str,
    head_commit: Option<gix::ObjectId>,
    change_id: changeid::ChangeId,
    change_born: Option<i64>,
    signer: Option<&'a sign::Signer>,
    /// Paths narrowed the close.
    partial: bool,
    commit_tree: gix::ObjectId,
    message: &'a str,
    /// A hook changed the message, so a differing one is fufu's to explain.
    hook_changed_message: bool,
    sig: &'a gix::actor::Signature,
}

/// The commit that lands: the open commit, when it is exactly what this
/// close would write — the pre-verb capture wrote it over this tree with
/// this message and this id, and the branch just moves onto it. Anything
/// that makes the landing commit differ is named, and the close mints one
/// of its own: the object is written up front because the plan needs its
/// sha. The second value is why the open commit did not land, when the
/// reason is fufu's to explain; a `-m` that differs from the description is
/// the user's own choice and gets no line.
fn landing_commit(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    landing: LandingCommit<'_>,
) -> Result<(gix::ObjectId, Option<Remint>)> {
    let LandingCommit {
        current_branch,
        head_commit,
        change_id,
        change_born,
        signer,
        partial,
        commit_tree,
        message,
        hook_changed_message,
        sig,
    } = landing;
    let now = ctx.now;
    let open_commit =
        open::current(repo, current_branch, ctx.pre_tree, head_commit)?.filter(|id| {
            repo.find_commit(*id)
                .ok()
                .is_some_and(|c| changeid::header_of(&c.data) == Some(change_id))
        });
    // `reuse` is whether the open commit lands; `reminted` is why not.
    let (reuse, reminted) = match open_commit {
        None => (false, None),
        Some(_) if signer.is_some() => (false, Some(Remint::Signed)),
        Some(_) if partial => (false, Some(Remint::Partial)),
        Some(id) => {
            let commit = repo.find_commit(id).map_err(Error::repo)?;
            let tree = commit.tree_id().map_err(Error::repo)?.detach();
            if tree != commit_tree {
                (false, Some(Remint::HookTree))
            } else if commit.message_raw_sloppy() != message.as_bytes() {
                (false, hook_changed_message.then_some(Remint::HookMessage))
            } else {
                (true, None)
            }
        }
    };
    let commit_id = match (open_commit, reuse) {
        (Some(id), true) => id,
        _ => {
            let parents: Vec<gix::ObjectId> = head_commit.into_iter().collect();
            let commit = gix::objs::Commit {
                tree: commit_tree,
                parents: parents.into(),
                // Authored at the change's birth, the way the open commit
                // is, so the date a commit shows is when the work began.
                author: refs::user_signature(repo, change_born.unwrap_or(now))?,
                committer: sig.clone(),
                encoding: None,
                message: message.to_string().into(),
                // The identity header sits inside the signed payload: the
                // signer pushes `gpgsig` after it.
                extra_headers: vec![changeid::header(&change_id)],
            };
            // A signing failure aborts here, with nothing but an unreferenced
            // object written — before the op-journal append and before any
            // ref moves, the same shape as every other pre-transaction
            // refusal.
            sign::write_user_commit(repo, signer, commit)?
        }
    };

    Ok((commit_id, reminted))
}

/// The CAS advance: the target branch moves onto the commit, as the user's
/// own signature in the reflog.
fn advance_branch(
    repo: &gix::Repository,
    target_ref: &str,
    commit_id: gix::ObjectId,
    head_commit: Option<gix::ObjectId>,
    subject: &str,
    sig: &gix::actor::Signature,
    now: i64,
) -> Result<()> {
    let expected = match head_commit {
        // Born: CAS against the exact tip the plan saw — the same tip a
        // fresh `-b` branch was just created at.
        Some(c) => {
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(c))
        }
        // Unborn: the close is the first commit.
        None => gix::refs::transaction::PreviousValue::MustNotExist,
    };
    let reflog_msg = match head_commit {
        Some(_) => format!("commit: {subject}"),
        None => format!("commit (initial): {subject}"),
    };
    let edit = refs::update_edit(target_ref, commit_id, expected, &reflog_msg)?;
    let time_str = format!("{now} +0000");
    let sig_ref = gix::actor::SignatureRef {
        name: sig.name.as_ref(),
        email: sig.email.as_ref(),
        time: &time_str,
    };
    match refs::commit_edits_as(repo, Some(edit), sig_ref)? {
        refs::EditOutcome::Applied => {}
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

    Ok(())
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
