//! The two verbs that aim a tree change at [`crate::rewrite::plan`]:
//! `ff absorb` folds the open change into a commit at a distance, and
//! `ff lift` runs the same reach backwards, taking paths out of a commit
//! and back into the open change.
//!
//! Neither verb writes a single file: nothing is added to or removed from
//! the working tree, and content is only reattributed between the open
//! change and a commit. The worktree is byte-identical before and after —
//! only refs, the index, and the operation log move.

use gix::prelude::ObjectIdExt;

use crate::error::{Error, Result};
use crate::model::{AbsorbOutcome, AbsorbReport, HeadState, LiftOutcome, LiftReport};
use crate::ops::record::observe_refs;
use crate::ops::{OpKind, OpRecord, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;

/// The branch HEAD sits on and its tip commit.
fn head_branch(repo: &gix::Repository, verb_noun: &str) -> Result<(String, gix::ObjectId)> {
    let head = crate::head::head_state(repo)?;
    match head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            Ok((name, tip))
        }
        HeadState::Unborn { .. } => Err(Error::coded(
            "target/unresolvable",
            format!("nothing is committed yet: there is nothing to {verb_noun}"),
            vec!["ff commit -m <msg>".into()],
        )),
        HeadState::Detached { .. } => Err(Error::coded(
            "repo/detached",
            "detached HEAD: there is no branch to carry the rewrite",
            vec!["ff switch <branch>".into()],
        )),
    }
}

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}

/// The tree of a commit, resolved through whichever repository handle is
/// given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// The first parent's tree, or the empty tree for a root commit.
fn parent_tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    let obj = repo.find_object(commit).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => tree_of(repo, gix::ObjectId::from_hex(hex).map_err(Error::repo)?),
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out — an absorb must be exact for the same reason a
/// commit is. The second result says the tree is clean.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<(gix::ObjectId, bool)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false))
}

/// `base` with the selected paths taking `other`'s content. An empty
/// `selectors` list selects everything, so the result is `other`.
fn filtered(
    repo: &gix::Repository,
    base: gix::ObjectId,
    other: gix::ObjectId,
    selectors: &[String],
) -> Result<gix::ObjectId> {
    if base == other {
        return Ok(base);
    }
    let lhs = repo.find_object(base).map_err(Error::repo)?.detach();
    let rhs = repo.find_object(other).map_err(Error::repo)?.detach();
    let mut recorder = gix::diff::tree::Recorder::default();
    gix::diff::tree(
        gix::objs::TreeRefIter::from_bytes(&lhs.data),
        gix::objs::TreeRefIter::from_bytes(&rhs.data),
        gix::diff::tree::State::default(),
        &repo.objects,
        &mut recorder,
    )
    .map_err(Error::repo)?;

    let mut editor = repo.edit_tree(base).map_err(Error::repo)?;
    use gix::diff::tree::recorder::Change as Rec;
    for record in recorder.records {
        match record {
            Rec::Addition {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue; // directories are not entries
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor
                    .upsert(path.as_str(), entry_mode.kind(), oid)
                    .map_err(Error::repo)?;
            }
            Rec::Deletion {
                entry_mode, path, ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor.remove(path.as_str()).map_err(Error::repo)?;
            }
            Rec::Modification {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor
                    .upsert(path.as_str(), entry_mode.kind(), oid)
                    .map_err(Error::repo)?;
            }
        }
    }
    Ok(editor.write().map_err(Error::repo)?.detach())
}

/// A three-way tree merge, resolved but not yet written: the caller probes
/// with a handle that writes nothing, and only then merges for real.
fn merge_into(
    repo: &gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<gix::merge::tree::Outcome<'_>> {
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    repo.merge_trees(base, ours, theirs, Default::default(), options)
        .map_err(Error::repo)
}

/// Fold the open change — or the part of it a path filter selected — into a
/// commit at a distance: `HEAD` by default, or the one named by `into`.
pub fn absorb(
    repo: &gix::Repository,
    into: Option<gix::ObjectId>,
    paths: Vec<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(AbsorbOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to absorb",
            vec![],
        ));
    }

    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge \
                 --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let (branch, tip) = head_branch(repo, "absorb into")?;

    let tip_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let (open_tree, clean) = open_tree(repo, tip_tree)?;
    if clean || open_tree == tip_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }

    // The tip's tree, with the selected paths taken from the worktree.
    let theirs = filtered(repo, tip_tree, open_tree, &paths)?;
    if theirs == tip_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }

    let target = into.unwrap_or(tip);
    let target_tree = tree_of(repo, target)?;
    let target_subject = subject(repo, target)?;

    // The target's new tree. When the target is the tip, it is `theirs`
    // directly — no merge runs, and it cannot conflict.
    let new_target_tree = if target == tip {
        theirs
    } else {
        let memory = repo.clone().with_object_memory();
        let probe = merge_into(&memory, tip_tree, target_tree, theirs)?;
        let conflicted = crate::futures::unresolved(&probe);
        if !conflicted.is_empty() {
            let target_short = short(repo, target);
            return Err(Error::coded(
                "held/rewrite-conflict",
                format!(
                    "folding your change into {target_short} \"{target_subject}\" conflicts in \
                     {}: nothing was absorbed",
                    rewrite::join_paths(&conflicted)
                ),
                vec![
                    "ff status".into(),
                    "ff absorb --into <rev>".into(),
                    "ff commit -m <msg>".into(),
                ],
            ));
        }
        let mut outcome = merge_into(repo, tip_tree, target_tree, theirs)?;
        outcome.tree.write().map_err(Error::repo)?.detach()
    };
    if new_target_tree == target_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }

    let plan = rewrite::plan(
        repo,
        target,
        tip,
        &rewrite::Change::Tree {
            tree: new_target_tree,
            message: None,
        },
        now,
    )?;
    let published = rewrite::published_count(repo, &branch, &plan)?;

    // Write-ahead: the planned table is the post-absorb world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let target_short = short(repo, target);
    let mut record = OpRecord::new(
        "absorb",
        format!("absorb into {target_short} on {branch}"),
        now,
    );
    record.argv = argv;
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    // Absorb writes no files, so the planned worktree is the one already
    // there, and the index is about to be rewritten to match the new tip.
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: ctx.pre_tree,
            index_tree: tree_of(repo, plan.new_tip)?,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("absorb: into {target_short}");
    let mut edits = Vec::new();
    for t in &plan.carried {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            continue;
        };
        let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
        let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
        edits.push(refs::update_edit(
            &t.name,
            new_id,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                old_id,
            )),
            &reflog_msg,
        )?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while absorbing; nothing was rewritten (re-run to absorb on the new \
                 tips)",
                vec![],
            ));
        }
    }

    crate::index::write_index_for_tree(repo, tree_of(repo, plan.new_tip)?)?;

    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = plan
        .carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();

    // The target's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop.
    let new_target = match plan.rewrites.iter().find(|r| r.old == target.to_string()) {
        Some(r) => Some(r.new.clone()),
        None if plan.dropped.iter().any(|d| d.old == target.to_string()) => None,
        None => return Err(Error::msg("the target was not in the rewrite plan")),
    };

    Ok((
        AbsorbOutcome::Absorbed(AbsorbReport {
            branch,
            into: target.to_string(),
            // The target is in `rewrites` exactly when it survived, so it is
            // subtracted from the restack exactly when it is there. Read
            // before `new_target` is moved into `new`.
            restacked: plan
                .rewrites
                .len()
                .saturating_sub(usize::from(new_target.is_some())),
            new: new_target,
            subject: target_subject,
            moved,
            published,
            paths,
            // The exact open tree from above, not `ctx.pre_tree`: the
            // capture floor may have size-capped a blob out of `pre_tree`
            // while the exact tree kept it, and comparing the capped tree
            // would report work still open when there is none.
            still_open: open_tree != tree_of(repo, plan.new_tip)?,
            dropped: plan.dropped.clone(),
        }),
        ctx,
    ))
}

/// Take paths out of a commit — `HEAD` by default, or the one named by
/// `from` — and back into the open change. The same reach as [`absorb`] run
/// backwards: the target keeps what it still introduces, its descendants
/// are restacked, and no file moves. A fully-lifted commit stays, as an
/// empty commit.
pub fn lift(
    repo: &gix::Repository,
    from: Option<gix::ObjectId>,
    paths: Vec<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(LiftOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to lift",
            vec![],
        ));
    }

    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge \
                 --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let (branch, tip) = head_branch(repo, "lift from")?;

    let target = from.unwrap_or(tip);
    let target_tree = tree_of(repo, target)?;
    let parent_tree = parent_tree_of(repo, target)?;

    // The target's new tree: its own, with the selected paths reverted to
    // the parent's content. No merge runs for the target itself — only its
    // descendants merge, inside the plan.
    let lifted = filtered(repo, target_tree, parent_tree, &paths)?;
    if lifted == target_tree {
        return Ok((
            LiftOutcome::NothingToLift {
                from: target.to_string(),
            },
            ctx,
        ));
    }

    let plan = rewrite::plan(
        repo,
        target,
        tip,
        &rewrite::Change::Tree {
            tree: lifted,
            message: None,
        },
        now,
    )?;
    let published = rewrite::published_count(repo, &branch, &plan)?;

    // Write-ahead: the planned table is the post-lift world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let target_short = short(repo, target);
    let mut record = OpRecord::new(
        "lift",
        format!("lift out of {target_short} on {branch}"),
        now,
    );
    record.argv = argv;
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    // Lift writes no files, so the planned worktree is the one already
    // there, and the index is about to be rewritten to match the new tip.
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: ctx.pre_tree,
            index_tree: tree_of(repo, plan.new_tip)?,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("lift: out of {target_short}");
    let mut edits = Vec::new();
    for t in &plan.carried {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            continue;
        };
        let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
        let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
        edits.push(refs::update_edit(
            &t.name,
            new_id,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                old_id,
            )),
            &reflog_msg,
        )?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while lifting; nothing was rewritten (re-run to lift on the new \
                 tips)",
                vec![],
            ));
        }
    }

    crate::index::write_index_for_tree(repo, tree_of(repo, plan.new_tip)?)?;

    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = plan
        .carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();

    // The target's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop.
    let new_target = match plan.rewrites.iter().find(|r| r.old == target.to_string()) {
        Some(r) => Some(r.new.clone()),
        None if plan.dropped.iter().any(|d| d.old == target.to_string()) => None,
        None => return Err(Error::msg("the target was not in the rewrite plan")),
    };

    Ok((
        LiftOutcome::Lifted(LiftReport {
            branch,
            from: target.to_string(),
            // The target is in `rewrites` exactly when it survived, so it is
            // subtracted from the restack exactly when it is there. Read
            // before `new_target` is moved into `new`.
            restacked: plan
                .rewrites
                .len()
                .saturating_sub(usize::from(new_target.is_some())),
            new: new_target,
            subject: subject(repo, target)?,
            moved,
            published,
            paths,
            dropped: plan.dropped.clone(),
        }),
        ctx,
    ))
}
