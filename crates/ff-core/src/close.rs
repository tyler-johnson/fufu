//! `ff commit` — the close. The working tree IS the open change; closing it
//! builds the `add -A` tree, writes an ordinary commit with the USER's
//! identity, advances the branch, and rewrites the index to match — the
//! next edit opens the next change. A clean tree with no message closes
//! nothing; a message (`-m` or pending description) closes as an empty
//! commit — no totally-empty commits.
//!
//! Ordering is write-ahead: capture-first snapshot → reconcile → hooks →
//! tree + message + plan → journal append → mutate (branch axis, ref CAS,
//! index, pending description). A crash after the append is labeled loudly
//! by the next reconcile.

use gix::prelude::ObjectIdExt;

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::hooks;
use crate::journal::{self, DescriptionTransition, OpKind, OpRecord, RefTransition};
use crate::model::{CommitOutcome, HeadState};
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
    /// Clock injection for tests.
    pub now: Option<i64>,
    /// The invoking argv, journaled verbatim.
    pub argv: Vec<String>,
}

/// Close the open change. `prov` names the mandatory pre-verb snapshot.
pub fn close(
    repo: &gix::Repository,
    opts: &CloseOptions,
    prov: &Provenance,
) -> Result<(CommitOutcome, journal::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::msg("bare repository: nothing to commit"));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::msg(format!(
            "a {op:?} is in progress: finish it with git (git commit / git merge --abort); fufu owns merges in a later phase"
        )));
    }

    let ctx = journal::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;

    let head = crate::head::head_state(repo)?;
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
            return Err(Error::msg(
                "detached HEAD: there is no branch to close onto (ff branch <name> to mint one)",
            ));
        }
    };

    // Read branchmeta early so emptiness can consult the pending description.
    let meta = branchmeta::read(repo, &current_branch)?;
    let pending = meta.pending_description.clone();

    // Emptiness first (git's order too): a clean tree with no message runs
    // no hooks and closes nothing.
    let mut scan = snaptree::scan(repo)?;
    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let effective = opts
        .message
        .clone()
        .or_else(|| pending.clone())
        .unwrap_or_default();
    let has_message = !normalize_message(&effective).is_empty();
    if scan.is_empty() && !has_message {
        return Ok((
            CommitOutcome::NothingToClose {
                branch: current_branch,
            },
            ctx,
        ));
    }

    // Hooks before the tree build — pre-commit hooks format files, so a
    // hook that ran invalidates the scan.
    if !opts.no_verify && hooks::pre_commit(repo)? {
        scan = snaptree::scan(repo)?;
        if scan.is_empty() && !has_message {
            return Ok((
                CommitOutcome::NothingToClose {
                    branch: current_branch,
                },
                ctx,
            ));
        }
    }

    // The close tree is exact: nothing is size-capped out of a commit.
    // When the scan is empty but a message exists, skip assemble — the
    // close tree IS the head tree (empty commit).
    let (tree_id, _skipped) = if scan.is_empty() {
        (head_tree, Vec::new())
    } else {
        snaptree::assemble(repo, head_tree, &scan, u64::MAX)?
    };
    if tree_id == head_tree && !has_message {
        return Ok((
            CommitOutcome::NothingToClose {
                branch: current_branch,
            },
            ctx,
        ));
    }

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
    let target_branch;
    let mut claim_from: Option<String> = None;
    let mut created_branch = false;
    match &opts.branch {
        None => target_branch = current_branch.clone(),
        Some(name) => {
            branch::validate_name(name)?;
            if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                return Err(Error::msg(format!("a branch named {name} already exists")));
            }
            if branch::is_anonymous(&current_branch) {
                claim_from = Some(current_branch.clone());
            } else {
                created_branch = true;
            }
            target_branch = name.clone();
        }
    }

    // The commit object, written up front — the plan needs its sha.
    let sig = refs::user_signature(repo, now)?;
    let parents: Vec<gix::ObjectId> = head_commit.into_iter().collect();
    let commit = gix::objs::Commit {
        tree: tree_id,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig.clone(),
        encoding: None,
        message: message.clone().into(),
        extra_headers: Vec::new(),
    };
    let commit_id = repo.write_object(&commit).map_err(Error::repo)?.detach();

    // Journal, write-ahead: the planned table is the post-close world.
    let target_ref = format!("refs/heads/{target_branch}");
    let mut planned = journal::observe_refs(repo)?;
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
        OpKind::Op,
        "commit",
        format!("commit on {target_branch}: {subject}"),
        now,
    );
    record.argv = opts.argv.clone();
    record.branch = Some(target_branch.clone());
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.refs = transitions;
    record.head = head_transition;
    record.description = pending.as_ref().map(|text| DescriptionTransition {
        branch: current_branch.clone(),
        old: Some(text.clone()),
        new: None,
    });
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    let mut pins = vec![commit_id];
    pins.extend(head_commit);
    if let Some(pre) = &ctx.pre_snapshot {
        pins.push(gix::ObjectId::from_hex(pre.as_bytes()).map_err(Error::repo)?);
    }
    journal::append(repo, &record, &planned, index_tree, &pins, now)?;

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
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::msg(format!(
                "{target_ref} moved while closing; nothing was committed (re-run to close on the new tip)"
            )));
        }
    }

    // The index becomes the new tree: nothing staged, next edit opens the
    // next change.
    crate::index::write_index_for_tree(repo, tree_id)?;

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

    // First close on a chainless repo: make sure gc guards exist (the
    // journal now pins history through refs/fufu/*).
    let _ = config::ensure_gc_config(repo);

    let files_changed = crate::snapshot::count_file_changes(repo, head_tree, tree_id)?;
    let short_id = commit_id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| commit_id.to_string()[..7].to_string());
    Ok((
        CommitOutcome::Closed {
            id: commit_id.to_string(),
            short_id,
            branch: target_branch,
            subject,
            files_changed,
            claimed_from: claim_from,
            pre_snapshot: ctx.pre_snapshot.clone(),
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
