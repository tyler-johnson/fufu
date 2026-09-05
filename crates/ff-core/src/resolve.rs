//! `ff resolve` — deal with a held rewrite. A held rewrite is a conflict
//! fufu chose not to interrupt you with; this is where you choose to deal
//! with it, and it materializes ALL of it at once: every surviving conflict
//! region lands in the working tree together, as ordinary labeled conflict
//! markers, in one editing session.
//!
//! Nothing moves. The branch ref is not retargeted and there is no arrival —
//! the parked change waits for `ff done` — so the session is not a branch but
//! one field of your own branch's metadata, and the hold STAYS: it is what
//! the session is resolving, and `ff done` needs it. The way back is one
//! `ff undo`; the way out is `--abandon`, which is also how you get out of a
//! session that is already open.

use crate::error::{Error, Result};
use crate::held::verb_of;
use crate::model::{AbandonedHold, HeadState, ReleasedReport, ResolveOutcome, ResolveReport};
use crate::ops::record::{HeldTransition, ResolveTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, StashEffect, verb};
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

/// The tree HEAD's commit carries, the same way `switch` asks it.
fn head_tree_of(repo: &gix::Repository, head: &HeadState) -> Option<gix::ObjectId> {
    match head {
        HeadState::Unborn { .. } => Some(gix::ObjectId::empty_tree(repo.object_hash())),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => {
            let id = gix::ObjectId::from_hex(commit.as_bytes()).ok()?;
            repo.find_commit(id)
                .ok()?
                .tree_id()
                .ok()
                .map(|t| t.detach())
        }
    }
}

/// The exact worktree tree: HEAD's tree with the scan assembled onto it,
/// nothing size-capped out — the same read `absorb` and `done` make of the
/// open change, which is what this has to reproduce.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<gix::ObjectId> {
    let scan = crate::snapshot::tree::scan(repo)?;
    if scan.is_empty() {
        return Ok(tip_tree);
    }
    let (tree_id, _skipped) = crate::snapshot::tree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok(tree_id)
}

/// Deal with the hold standing on the current branch: open a resolution
/// session over it, release it when the world moved out of the conflict, or
/// drop it with `abandon`. See the module docs.
pub fn resolve(
    repo: &gix::Repository,
    abandon: bool,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to resolve",
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

    let head = crate::head::head_state(repo)?;
    let (branch, tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (name.clone(), tip)
        }
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "nothing is committed yet: there is nothing to resolve",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: a resolution session needs a branch to stand on",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    let open = crate::held::resolving(repo, &branch)?;

    // Re-materializing over an open session would overwrite the very edits
    // the session exists to collect, so a second resolve refuses — and
    // `--abandon` is exactly how you get out of the first one.
    if open.is_some() && !abandon {
        return Err(Error::coded(
            "held/resolving",
            format!(
                "a resolution is already open on {branch}: its conflicts are in your working copy"
            ),
            vec![
                "ff done".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }

    let held = crate::held::of(repo, &branch)?;

    if abandon {
        return abandon_hold(
            repo,
            ctx,
            (prov, argv, now),
            &head,
            &branch,
            tip,
            (open, held),
        );
    }

    let held = held.ok_or_else(|| {
        Error::coded(
            "held/none",
            format!("nothing is held on {branch}: there is no pending rewrite to resolve"),
            vec!["ff status".into(), "ff log".into()],
        )
    })?;

    // Ask again against the repository as it stands. A `held/expired` passes
    // straight through — the hold has outlived its meaning, and saying so is
    // the answer.
    let replan = crate::held::replan(repo, &held)?;

    let Some(conflict) = crate::rewrite::conflict(repo, replan.target, replan.tip, &replan.change)?
    else {
        // The world moved and the rewrite is clean now. A hold is a cache
        // over "this rewrite conflicts", and a cache entry that no longer
        // describes anything is not an event — so the clear is a plain
        // metadata write with no operation, and the verb that recorded the
        // hold lands the rewrite when it is re-run.
        crate::held::set(repo, &branch, None)?;
        return Ok((
            ResolveOutcome::Released(ReleasedReport {
                branch,
                verb: verb_of(&held),
            }),
            ctx,
        ));
    };
    // The size of the whole rewrite, tangled commit included: the label
    // names the stack, not this attempt.
    let of = conflict.of;

    // A filtered absorb or lift rewrites only the paths it selected. Changes
    // standing OUTSIDE that filter are not in the chain, and the markers
    // about to go in would overwrite them — work lost, so it refuses before
    // touching anything. An unfiltered rewrite selects every path, and
    // `restack` and `done` fold the whole tree into their own, so neither
    // can lose what it did not select.
    let filtered = match &held.intent {
        crate::held::Intent::Absorb { paths, .. } | crate::held::Intent::Lift { paths, .. }
            if !paths.is_empty() =>
        {
            Some(paths)
        }
        _ => None,
    };
    if let Some(filter) = filtered {
        let scan = crate::snapshot::tree::scan(repo)?;
        let mut outside: Vec<String> = scan
            .staged_upserts
            .iter()
            .map(|(path, _, _)| path.clone())
            .chain(scan.staged_deletes.iter().cloned())
            .chain(scan.rehash.iter().cloned())
            .chain(scan.untracked.iter().cloned())
            .chain(scan.wt_deletes.iter().cloned())
            .filter(|path| !filter.contains(path))
            .collect();
        outside.sort();
        outside.dedup();
        if !outside.is_empty() {
            return Err(Error::coded(
                "held/unsupported",
                format!(
                    "the held {} selected paths, and your open change also touches {}: \
                     resolving would overwrite what it did not select",
                    verb_of(&held),
                    crate::rewrite::join_paths(&outside),
                ),
                vec![
                    "ff commit -m <msg>".into(),
                    "ff resolve --abandon".into(),
                    "ff status".into(),
                ],
            ));
        }
    }

    // Replay it all the way through, carrying the conflicts as literal
    // marker content, and read back the regions standing in the result.
    let chain = crate::rewrite::chain(repo, replan.target, replan.tip, &replan.change, &[])?;
    let regions = crate::rewrite::regions(repo, &chain)?;

    // Park the open change only when the rewrite CARRIES it: a `restack`
    // moves commits underneath the change, so it must be stowed to make room
    // for the markers and come back on top at `ff done`. `done`, `absorb`
    // and `lift` have already FOLDED the working tree into the chain's own
    // trees — that is what their replans read it for — so the change is in
    // the commits being written, and parking it would bring it back at
    // landing and apply it twice. The least obvious rule in the feature, and
    // the one a change is most likely to undo by accident.
    let park_plan = if matches!(held.intent, crate::held::Intent::Restack { .. }) {
        stash::plan_park(repo, &head, now)?
    } else {
        None
    };

    // The working tree as it stands, before the markers take its place. A
    // `done`, `absorb` or `lift` plan is derived from the open change, and
    // this is the last moment it can be read: recording it is what lets
    // `ff done` replan to the same plan and mean "the world moved" when it
    // does not.
    let open = head_tree_of(repo, &head)
        .map(|tip_tree| open_tree(repo, tip_tree))
        .transpose()?
        .map(|tree| tree.to_string());

    let session = crate::held::Resolve {
        hold: held.clone(),
        from: chain.tree.to_string(),
        steps: chain.steps.iter().map(|s| s.subject.clone()).collect(),
        open,
    };

    // Write the op ahead, then mutate — the journal describes the whole
    // resolve up front. No branch ref moves: HEAD stays where it stands and
    // there is no arrival, so the only ref work is the park's, recorded the
    // way `switch` records the same park.
    let mut planned = observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let mut effects: Vec<StashEffect> = Vec::new();
    if let Some(plan) = &park_plan {
        let mut stash_lines: Vec<gix::ObjectId> =
            crate::refs::read_ref_log(repo, stash::STASH_REF)?
                .iter()
                .map(|l| l.new)
                .collect();
        stash_lines.push(plan.wip_commit);
        planned
            .refs
            .insert(stash::parked_ref(&branch), plan.wip_commit.to_string());
        transitions.push(RefTransition {
            name: stash::parked_ref(&branch),
            old: None,
            new: Some(plan.wip_commit.to_string()),
        });
        effects.push(StashEffect::Push {
            branch: branch.clone(),
            stash: plan.wip_commit.to_string(),
        });
        match stash_lines.last() {
            Some(tip) => {
                planned.refs.insert("refs/stash".into(), tip.to_string());
            }
            None => {
                planned.refs.remove("refs/stash");
            }
        }
    }

    let mut record = OpRecord::new(
        "resolve",
        format!(
            "resolve the held {} on {}: {} region(s)",
            verb_of(&held),
            branch,
            regions.len()
        ),
        now,
    );
    record.argv = argv.clone();
    record.refs = transitions;
    record.stash = effects;
    record.resolving = Some(ResolveTransition {
        branch: branch.clone(),
        old: None,
        new: Some(session.clone()),
    });
    // The park's WIP commit pinned too, the way `switch` pins its park: an
    // open session must not be able to outlive the change it parked.
    let mut pins = vec![chain.tree];
    if let Some(plan) = &park_plan {
        pins.push(plan.wip_commit);
    }
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // The planned END state, which is the whole of undo's rule: the
            // markers are the working tree this op leaves behind.
            tree: chain.tree,
            index_tree: chain.tree,
            branch: branch.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Mutate: park, index, worktree — the `switch` order minus the retarget
    // and the arrival.
    let parked = match &park_plan {
        Some(plan) => {
            stash::execute_park(repo, plan)?;
            Some(plan.wip_commit.to_string())
        }
        None => None,
    };
    crate::index::write_index_for_tree(repo, chain.tree)?;
    let from_tree = park_plan
        .as_ref()
        .map(|p| p.head_tree)
        .unwrap_or_else(|| head_tree_of(repo, &head).unwrap_or(chain.tree));
    let everything = |_: &str| true;
    worktree::apply_tree_transition(repo, from_tree, chain.tree, &everything)?;

    // The hold STAYS — it is what the session is resolving, and `ff done`
    // needs it — and the session is recorded where the hold lives.
    crate::held::set_resolving(repo, &branch, Some(session))?;

    let mut files: Vec<String> = regions.iter().map(|r| r.path.clone()).collect();
    files.sort();
    files.dedup();

    Ok((
        ResolveOutcome::Opened(ResolveReport {
            branch,
            verb: verb_of(&held),
            files,
            regions: regions.len(),
            steps: chain.steps.len(),
            of,
            tangled: chain.tangled.map(|t| t.subject),
            parked,
        }),
        ctx,
    ))
}

/// `--abandon`: drop the hold, and the session with it if one is open — one
/// operation, so one `ff undo` puts both back. (Named for what it does,
/// since the flag it serves shadows it.)
fn abandon_hold(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    branch: &str,
    tip: gix::ObjectId,
    standing: (Option<crate::held::Resolve>, Option<crate::held::Held>),
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    let (open, held) = standing;
    let (prov, argv, now) = invocation;
    let argv = &argv;
    if open.is_none() && held.is_none() {
        return Err(Error::coded(
            "held/none",
            format!("nothing is held on {branch}: there is no pending rewrite to resolve"),
            vec!["ff status".into(), "ff log".into()],
        ));
    }
    let was_resolving = open.is_some();

    // The session carries a copy of the hold it was opened on, so a hold
    // cleared underneath it can still name its verb.
    let verb = match (held.as_ref(), open.as_ref()) {
        (Some(h), _) => verb_of(h),
        (None, Some(session)) => verb_of(&session.hold),
        (None, None) => unreachable!("the empty abandon was refused above"),
    };

    // What the worktree goes back to. A restack's change was parked, so it
    // comes back out of the stash and the entry is consumed. A `done`,
    // `absorb` or `lift` parked nothing — it folded the change into the
    // chain's trees instead — so what stood there before the markers is the
    // working tree the session recorded, never HEAD's, which would be an
    // abandon that quietly threw the change away. The index goes back to
    // HEAD's tree, so the restored change reads as open rather than staged.
    // No session, nothing to put back.
    let empty = gix::ObjectId::empty_tree(repo.object_hash());
    let head_tree = head_tree_of(repo, head).unwrap_or(tip_tree(repo, tip));
    let parked = if open.is_some() {
        stash::parked_entry(repo, branch)?
    } else {
        None
    };
    let restore: Option<(gix::ObjectId, gix::ObjectId, gix::ObjectId)> =
        match (open.as_ref(), parked) {
            (Some(_), Some(p)) => {
                let stash = stash::read_stash_commit(repo, p)?;
                Some((stash.wip_tree, stash.index_tree, stash.untracked_tree))
            }
            (Some(session), None) => {
                let folded = session
                    .open
                    .as_deref()
                    .map(|hex| gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo))
                    .transpose()?;
                match folded {
                    Some(tree) => Some((tree, head_tree, empty)),
                    None => Some((head_tree, head_tree, empty)),
                }
            }
            (None, _) => None,
        };
    // The state the op leaves the worktree in. When nothing is restored the
    // tree is untouched, so its planned end state is what the preamble
    // recorded — the tip's tree would claim a clean state a dirty tree is not.
    let (end_tree, end_index) = match restore.as_ref() {
        Some((w, i, _)) => (*w, *i),
        None => (ctx.pre_tree, crate::index::tree_from_index(repo)?),
    };
    // Where the markers stood when the session opened: the starting point of
    // the full replacement below. Parsed now, because the session value is
    // consumed by the record a moment later.
    let marker_tree = open
        .as_ref()
        .map(|session| gix::ObjectId::from_hex(session.from.as_bytes()).map_err(Error::repo))
        .transpose()?;

    // Write the op ahead: both clears in one record, and the consumed
    // parking the way `switch` journals an arrival.
    let mut planned = observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let mut effects: Vec<StashEffect> = Vec::new();
    if let Some(p) = parked {
        let mut stash_lines: Vec<gix::ObjectId> =
            crate::refs::read_ref_log(repo, stash::STASH_REF)?
                .iter()
                .map(|l| l.new)
                .collect();
        if let Some(pos) = stash_lines.iter().rposition(|s| *s == p) {
            stash_lines.remove(pos);
        }
        let parked_ref = stash::parked_ref(branch);
        planned.refs.remove(&parked_ref);
        transitions.push(RefTransition {
            name: parked_ref,
            old: Some(p.to_string()),
            new: None,
        });
        effects.push(StashEffect::Drop {
            branch: branch.to_string(),
            stash: p.to_string(),
        });
        match stash_lines.last() {
            Some(t) => {
                planned.refs.insert("refs/stash".into(), t.to_string());
            }
            None => {
                planned.refs.remove("refs/stash");
            }
        }
    }

    let mut record = OpRecord::new(
        "resolve",
        if open.is_some() {
            format!("abandon the resolution of the held {verb} on {branch}")
        } else {
            format!("drop the held {verb} on {branch}")
        },
        now,
    );
    record.argv = argv.to_vec();
    record.refs = transitions;
    record.stash = effects;
    record.held = Some(HeldTransition {
        branch: branch.to_string(),
        old: held,
        new: None,
    });
    record.resolving = Some(ResolveTransition {
        branch: branch.to_string(),
        old: open,
        new: None,
    });
    let pins = vec![end_tree];
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // The state this op leaves behind: whatever the worktree was put
            // back to, or the untouched tree when there was nothing to
            // restore.
            tree: end_tree,
            index_tree: end_index,
            branch: branch.to_string(),
            base: crate::snapshot::chain::base_commit(head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Mutate. The tree is replaced in full — clear everything the markers
    // touched, then lay the target back — because a session in progress may
    // have edited the markers, and a diff against the target alone would
    // leave those edits standing.
    if let Some((wip, index, untracked)) = restore {
        let everything = |_: &str| true;
        let marker_tree = marker_tree.expect("a restore implies a session");
        worktree::apply_tree_transition(repo, marker_tree, empty, &everything)?;
        worktree::apply_tree_transition(repo, empty, wip, &everything)?;
        worktree::apply_tree_transition(repo, empty, untracked, &everything)?;
        crate::index::write_index_for_tree(repo, index)?;
        if let Some(p) = parked {
            stash::drop_stash_entry(repo, p)?;
            crate::refs::delete_ref(repo, &stash::parked_ref(branch), p, now)?;
        }
    }

    crate::held::set(repo, branch, None)?;
    crate::held::set_resolving(repo, branch, None)?;

    Ok((
        ResolveOutcome::Abandoned(AbandonedHold {
            branch: branch.to_string(),
            verb,
            was_resolving,
        }),
        ctx,
    ))
}

fn tip_tree(repo: &gix::Repository, tip: gix::ObjectId) -> gix::ObjectId {
    repo.find_commit(tip)
        .expect("a born branch has a tip")
        .tree_id()
        .expect("a commit has a tree")
        .detach()
}
