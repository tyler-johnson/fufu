//! `ff done` ends an editing session opened by `ff edit`: the edited commit
//! is amended with the session's content, what waited ahead of it on the
//! branch it will replay onto, the worktree lands back on that branch, and
//! the session branch is gone. This is **one operation** — the refs, HEAD,
//! the worktree and the change `ff edit` parked all move together, so a
//! single `ff undo` takes the whole session back. That is why this fuses the
//! rewrite and the return rather than composing `ff restack` with
//! `ff switch`: two operations would need two undos, and the first of them
//! would land on a state that still holds the session open.
//!
//! The rewrite reaches [`crate::rewrite::plan`] directly, with
//! [`crate::rewrite::Change::Tree`] on the session branch's own tip and the
//! landing branch's tip as the range end — never a call into
//! [`crate::restack`], whose merge-base arithmetic is built for a branch
//! standing behind its base, not for a branch that sits *at* the commit
//! being amended: aiming restack here would replay the edited commit a
//! second time.
//!
//! `--abandon` is the escape hatch: it drops the session's uncommitted edits
//! (parked, never discarded) rather than landing them, and it works in
//! exactly the states where `ff done` refuses — a session that gained
//! commits of its own, or one whose anchor fell out of the landing branch's
//! history, both fold away without complaint under `--abandon`.

use gix::prelude::ObjectIdExt;

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::futures;
use crate::model::{AbandonReport, ArrivalReport, DoneOutcome, DoneReport, HeadState};
use crate::ops::record::{SessionTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, StashEffect, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::stash::{self, ArrivePlan};

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
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

/// A commit's full message, through the object handle — the raw `CommitRef`
/// message is what the amend compares and lands.
fn message_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let obj = repo.find_object(commit).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(commit_ref.message.to_string())
}

/// A commit's first parent, or `None` for a root.
fn first_parent(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<gix::ObjectId>> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => Ok(Some(gix::ObjectId::from_hex(hex).map_err(Error::repo)?)),
        None => Ok(None),
    }
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out. The second result says the tree is clean.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<(gix::ObjectId, bool)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false))
}

fn session_none() -> Error {
    Error::coded(
        "session/none",
        "no editing session is running",
        vec!["ff edit <rev>".into(), "ff status".into()],
    )
}

/// End the editing session running on HEAD: land it (amend, replay, return),
/// or drop it with `abandon`. See the module docs.
pub fn done(
    repo: &gix::Repository,
    abandon: bool,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(DoneOutcome, verb::VerbContext)> {
    // 1. Guards.
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to finish",
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

    // 2.
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    // 3. The session.
    let head = crate::head::head_state(repo)?;
    let (session_branch, session_tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (name.clone(), tip)
        }
        HeadState::Unborn { .. } | HeadState::Detached { .. } => return Err(session_none()),
    };
    let meta = branchmeta::read(repo, &session_branch)?;
    let Some(sess) = meta.session.clone() else {
        return Err(session_none());
    };

    let onto = sess.onto.clone();
    let anchor = gix::ObjectId::from_hex(sess.at.as_bytes()).map_err(Error::repo)?;
    let anchor_short = short(repo, anchor);
    let anchor_subject = subject(repo, anchor)?;

    // 4a. A commit landed on top of the session — the branch grew — refuses:
    // finishing would amend the anchor and leave the landed commit behind.
    // A rewrite of the anchor in place (`ff absorb`, `ff lift`, `ff describe`)
    // does not grow the branch: the rewrite copies the parent list, so the
    // first-parent comparison accepts it. Landing path only — abandon drops
    // the session anyway.
    if !abandon {
        let tip_parent = first_parent(repo, session_tip)?;
        let anchor_parent = first_parent(repo, anchor)?;
        if tip_parent != anchor_parent {
            return Err(Error::coded(
                "session/moved",
                format!(
                    "{session_branch} has commits of its own since the session opened: \
                     finishing would amend {anchor_short} and leave them behind"
                ),
                vec!["ff done --abandon".into(), "ff undo".into()],
            ));
        }
    }

    // 4b. `onto` must still exist — both paths.
    let onto_ref = format!("refs/heads/{onto}");
    let onto_tip = refs::ref_target(repo, &onto_ref)?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("{onto}, the branch this session replays onto, no longer exists"),
            vec!["ff switch <branch>".into()],
        )
    })?;
    branch::guard_other_worktrees(repo, &onto)?;

    // 4c. The edited commit must still be in `onto`'s history — landing path only.
    if !abandon {
        let bases: Vec<gix::ObjectId> = repo
            .merge_bases_many(anchor, &[onto_tip])
            .map_err(Error::repo)?
            .into_iter()
            .map(|id| id.detach())
            .collect();
        if !bases.contains(&anchor) {
            return Err(Error::coded(
                "session/unreachable",
                format!(
                    "{anchor_short} \"{anchor_subject}\" is no longer in {onto}'s history: this \
                     session has nothing to land onto"
                ),
                vec!["ff done --abandon".into(), "ff log".into()],
            ));
        }
    }

    let session_ref = format!("refs/heads/{session_branch}");
    let session_tip_tree = tree_of(repo, session_tip)?;

    // 5. The landing path: the rewrite. Planning only — no ref moves yet.
    // The anchor is what sits in `onto`'s history and therefore what can be
    // replayed onto; the tip only says what the content became.
    let mut rewrite_plan: Option<rewrite::RewritePlan> = None;
    let mut unchanged = false;
    let mut worktree_tree: Option<gix::ObjectId> = None;
    if !abandon {
        let assembled = open_tree(repo, session_tip_tree)?.0;
        let anchor_tree = tree_of(repo, anchor)?;
        // The amend is one tree change carrying both halves that differ
        // from the anchor: the content the worktree holds, and the tip's
        // message if a reword landed on the tip under us. A pure reword is
        // the same change with the anchor's own tree, so no merge runs
        // anywhere. Neither differs and there is nothing to land.
        let tip_message = message_of(repo, session_tip)?;
        let anchor_message = message_of(repo, anchor)?;
        if assembled == anchor_tree && tip_message == anchor_message {
            unchanged = true;
        } else {
            rewrite_plan = Some(rewrite::plan(
                repo,
                anchor,
                onto_tip,
                &rewrite::Change::Tree {
                    tree: assembled,
                    message: (tip_message != anchor_message).then_some(tip_message.into()),
                },
                now,
            )?);
        }
        worktree_tree = Some(assembled);
    }

    // 6. The abandon path: park what is uncommitted. Planning only.
    let park_plan = if abandon {
        stash::plan_park(repo, &head, now)?
    } else {
        None
    };

    // `plan.carried` holds the session branch too (the map rewrote it): that
    // entry becomes a deletion, not a ref update, below.
    let carried: Vec<RefTransition> = rewrite_plan
        .as_ref()
        .map(|plan| {
            plan.carried
                .iter()
                .filter(|t| t.name != session_ref)
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // The anchor's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop. No plan at
    // all means the session changed nothing, and a session that changed
    // nothing did not drop its commit.
    let amended = match &rewrite_plan {
        Some(plan) => match plan.rewrites.iter().find(|r| r.old == anchor.to_string()) {
            Some(r) => Some(r.new.clone()),
            None if plan.dropped.iter().any(|d| d.old == anchor.to_string()) => None,
            None => {
                return Err(Error::msg(
                    "the session's commit was not in the rewrite plan",
                ));
            }
        },
        None => Some(anchor.to_string()),
    };

    let (new_onto_tip, replayed, published, dropped) = match &rewrite_plan {
        Some(plan) => {
            // The anchor is in `rewrites` exactly when `amended` is `Some`,
            // so the count subtracts it exactly when it is there.
            let replayed = plan
                .rewrites
                .len()
                .saturating_sub(usize::from(amended.is_some()));
            let published = rewrite::published_count(repo, &onto, plan)?;
            (plan.new_tip, replayed, published, plan.dropped.clone())
        }
        None => (onto_tip, 0, 0, Vec::new()),
    };
    let published_on = rewrite::tracking_name(repo, &onto)?;
    let new_onto_tree = tree_of(repo, new_onto_tip)?;

    // 7. The return trip, planned before anything moves.
    let arrive_plan = stash::plan_arrival(repo, &onto, new_onto_tip, new_onto_tree)?;

    let mut planned = observe_refs(repo)?;
    let head_old = planned.head.clone();
    planned.head = format!("ref:{onto_ref}");
    planned.refs.remove(&session_ref);
    for t in &carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let mut refs_transitions: Vec<RefTransition> = carried.clone();
    refs_transitions.push(RefTransition {
        name: session_ref.clone(),
        old: Some(session_tip.to_string()),
        new: None,
    });

    let mut stash_lines: Vec<gix::ObjectId> = refs::read_ref_log(repo, stash::STASH_REF)?
        .iter()
        .map(|l| l.new)
        .collect();
    let mut effects: Vec<StashEffect> = Vec::new();
    if let Some(plan) = &park_plan {
        stash_lines.push(plan.wip_commit);
        effects.push(StashEffect::Push {
            branch: session_branch.clone(),
            stash: plan.wip_commit.to_string(),
        });
        // The parked ref this park writes is demoted within the same
        // operation — the branch it names is about to be deleted — so it
        // leaves no net transition on the recorded table; only the stash
        // push above is a real, persisting change.
    }
    match &arrive_plan {
        ArrivePlan::Restore { stash: sha, .. } => {
            if let Some(pos) = stash_lines.iter().rposition(|s| s == sha) {
                stash_lines.remove(pos);
            }
            planned.refs.remove(&stash::parked_ref(&onto));
            refs_transitions.push(RefTransition {
                name: stash::parked_ref(&onto),
                old: Some(sha.to_string()),
                new: None,
            });
            effects.push(StashEffect::Drop {
                branch: onto.clone(),
                stash: sha.to_string(),
            });
        }
        ArrivePlan::Invalidate { stash: sha } => {
            planned.refs.remove(&stash::parked_ref(&onto));
            refs_transitions.push(RefTransition {
                name: stash::parked_ref(&onto),
                old: Some(sha.to_string()),
                new: None,
            });
        }
        ArrivePlan::None | ArrivePlan::Conflict { .. } => {}
    }
    match stash_lines.last() {
        Some(tip) => {
            planned
                .refs
                .insert(stash::STASH_REF.to_string(), tip.to_string());
        }
        None => {
            planned.refs.remove(stash::STASH_REF);
        }
    }

    let (end_tree, end_index) = match &arrive_plan {
        ArrivePlan::Restore {
            target_wip,
            target_index,
            ..
        } => (*target_wip, *target_index),
        _ => (new_onto_tree, new_onto_tree),
    };

    let summary = if abandon {
        format!("done --abandon: {anchor_short} on {onto}")
    } else {
        format!("done: {anchor_short} on {onto}")
    };
    let mut record = OpRecord::new("done", summary, now);
    record.argv = argv;
    record.head = Some((head_old, format!("ref:{onto_ref}")));
    record.refs = refs_transitions;
    record.stash = effects;
    if let Some(plan) = &rewrite_plan {
        record.rewrites = plan.rewrites.clone();
    }
    record.edit_session = Some(SessionTransition {
        branch: session_branch.clone(),
        old: Some(sess.clone()),
        new: None,
    });

    let mut pins: Vec<gix::ObjectId> = Vec::new();
    if let Some(plan) = &rewrite_plan {
        for r in &plan.rewrites {
            pins.push(gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo)?);
        }
    }
    pins.push(session_tip);
    pins.push(onto_tip);
    match &arrive_plan {
        ArrivePlan::Restore { stash, .. }
        | ArrivePlan::Conflict { stash, .. }
        | ArrivePlan::Invalidate { stash } => pins.push(*stash),
        ArrivePlan::None => {}
    }
    if let Some(plan) = &park_plan {
        pins.push(plan.wip_commit);
    }

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: end_index,
            branch: onto.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 8. Mutate, in order: park (abandon, if dirty), HEAD, refs, index,
    // worktree, arrive, metadata, futures caches.
    let stashed = match &park_plan {
        Some(plan) => {
            stash::execute_park(repo, plan)?;
            Some(plan.wip_commit.to_string())
        }
        None => None,
    };

    branch::retarget_head(repo, &onto_ref, now)?;

    let reflog_msg = format!("done: onto {onto}");
    let mut edits = Vec::new();
    for t in &carried {
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
    edits.push(refs::delete_edit(&session_ref, session_tip)?);
    if let Some(plan) = &park_plan {
        edits.push(refs::delete_edit(
            &stash::parked_ref(&session_branch),
            plan.wip_commit,
        )?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while finishing the session; nothing further was changed (re-run ff \
                 done)",
                vec![],
            ));
        }
    }

    crate::index::write_index_for_tree(repo, new_onto_tree)?;

    // `from_tree` is what the worktree holds right now: the amended tree on
    // the landing path, the session tip's tree on the abandon path (park
    // already reset it there, and a clean abandon never dirtied it).
    let from_tree = if abandon {
        session_tip_tree
    } else {
        worktree_tree.expect("computed on the landing path above")
    };
    let everything = |_: &str| true;
    let transition =
        crate::worktree::apply_tree_transition(repo, from_tree, new_onto_tree, &everything)?;

    let arrival = stash::execute_arrival(repo, &onto, &arrive_plan, new_onto_tree, now)?;
    let arrival_report = match arrival {
        stash::Arrival::None => ArrivalReport::None,
        stash::Arrival::Restored { stash, files } => ArrivalReport::Restored { stash, files },
        stash::Arrival::Conflicted { stash, paths } => ArrivalReport::StillParked { stash, paths },
        stash::Arrival::Invalidated { stash } => ArrivalReport::Invalidated { stash },
    };

    // Metadata: clear the session, leave the file and `forked_from` alone.
    // Undo restores `session` from the recorded transition, not from the
    // file, so deleting the file wholesale would come back missing
    // `forked_from`.
    let mut session_meta = branchmeta::read(repo, &session_branch)?;
    session_meta.session = None;
    branchmeta::write(repo, &session_branch, &session_meta)?;

    // Futures caches: best-effort, as restack.rs removes them.
    let _ = futures::cache::remove(repo, &onto);
    for t in &carried {
        if let Some(name) = t.name.strip_prefix("refs/heads/") {
            let _ = futures::cache::remove(repo, name);
        }
    }

    // 9. The report.
    let mut files = transition.written.len() + transition.deleted.len();
    if let ArrivalReport::Restored { files: f, .. } = &arrival_report {
        files += f.len();
    }

    if abandon {
        Ok((
            DoneOutcome::Abandoned(AbandonReport {
                session: session_branch,
                editing: anchor.to_string(),
                subject: anchor_subject,
                onto,
                stashed,
                arrival: arrival_report,
                files,
            }),
            ctx,
        ))
    } else {
        let moved: Vec<String> = carried
            .iter()
            .filter(|t| t.name != onto_ref)
            .map(|t| {
                t.name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&t.name)
                    .to_string()
            })
            .collect(); // carried is already sorted by ref name
        Ok((
            DoneOutcome::Done(DoneReport {
                session: session_branch,
                editing: anchor.to_string(),
                amended,
                subject: anchor_subject,
                onto,
                replayed,
                moved,
                new_tip: new_onto_tip.to_string(),
                unchanged,
                published,
                published_on,
                arrival: arrival_report,
                files,
                dropped,
            }),
            ctx,
        ))
    }
}
