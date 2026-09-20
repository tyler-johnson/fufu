//! A merge: one commit with two parents, the branch's tip first, taking
//! another tree in the way the branch's own history already does. `ff
//! resolve` is its first door — a branch behind a base its commits already
//! hold a merge of — and `ff merge <branch>` will be its second; both plan
//! here, hold here through `held::Intent::Merge`, and commit here.
//!
//! The merge is fufu's, not git's: a change id, the user's signature, the
//! signing configuration every commit fufu writes honors, one operation
//! `ff undo` takes back, and fufu's hold and resolution session in place of
//! git's index when the auto-merge conflicts. No hook runs for it. Nothing
//! cascades: the branches stacked above sit on commits the merge leaves in
//! place, as they do after `ff commit`.

use crate::changeid;
use crate::error::{Error, Result};
use crate::futures;
use crate::held::{self, Held};
use crate::model::{ArrivalReport, HeadState, MergeReport};
use crate::ops::record::{HeldTransition, RefTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, verb};
use crate::park::{self, ArrivePlan};
use crate::refs;
use crate::restack::{self, Onto};
use crate::rewrite;
use crate::sign;
use crate::snapshot::Provenance;
use crate::stash;

/// A merge decided and not yet written: the branch, its tip, what it takes
/// in, the message, the replan `ff done` re-derives, and the verdict.
pub(crate) struct MergePlan {
    pub branch: String,
    pub tip: gix::ObjectId,
    pub onto: Onto,
    /// The commit's message, normalized.
    pub message: String,
    /// `onto`'s tip is already beneath `tip`: there is nothing to merge, and
    /// a hold asking for it is moot.
    pub already_in: bool,
    /// The replan, the triple `rewrite::chain` and `rewrite::conflict` take:
    /// what a hold re-derives, so the hold the door records and the replan
    /// `ff done` makes cannot disagree.
    pub replan: held::Replan,
    /// `rewrite::conflict` on the merge chain; `None` when the auto-merge is
    /// clean.
    pub conflict: Option<rewrite::Conflict>,
    /// The clean tree, from `rewrite::chain` on the real handle so it is
    /// kept for the commit. `None` when the auto-merge conflicts.
    pub tree: Option<gix::ObjectId>,
}

/// The commit's default subject: `merge <base> into <branch>`.
fn default_message(onto: &Onto, branch: &str) -> String {
    format!("merge {} into {branch}", onto.name)
}

/// The tip of `branch`, or `branch/not-found`.
fn tip_of(repo: &gix::Repository, branch: &str) -> Result<gix::ObjectId> {
    refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {branch}"),
            vec![],
        )
    })
}

/// The replan for a merge of `onto` into `branch`, with the tip it read and
/// whether `onto` is already beneath it. A tip beneath `onto` — someone
/// restacked by hand under the hold — has nothing to merge, and the hold is
/// stale.
fn replan_onto(
    repo: &gix::Repository,
    branch: &str,
    onto: &Onto,
) -> Result<(gix::ObjectId, bool, held::Replan)> {
    let tip = tip_of(repo, branch)?;
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(tip, &[onto.tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Err(Error::coded(
            "merge/unrelated",
            format!(
                "{branch} and {} have no common ancestor: there is nothing to merge",
                onto.name
            ),
            vec!["ff log".into()],
        ));
    }
    // Up to date before beneath: equal tips are "already in", not "stale".
    let already_in = bases.contains(&onto.tip);
    if !already_in && bases.contains(&tip) {
        return Err(Error::msg(format!(
            "{branch} is beneath {}, so there is nothing to merge",
            onto.name
        )));
    }
    Ok((
        tip,
        already_in,
        held::Replan {
            target: tip,
            tip,
            change: rewrite::Change::Merge {
                other: onto.tip,
                subject: onto.name.clone(),
            },
        },
    ))
}

/// Turn a held merge back into a plan against the repository as it stands:
/// `onto` is a ref name, resolved fresh, as a restack's is. A gone ref or
/// branch, or a tip beneath `onto`, is an error `held::replan_at` reports as
/// `held/expired`.
pub(crate) fn replan(repo: &gix::Repository, branch: &str, onto: &str) -> Result<held::Replan> {
    let onto = restack::resolve_onto(repo, onto)?;
    replan_onto(repo, branch, &onto).map(|(_, _, replan)| replan)
}

/// Plan the merge of `onto` into `branch`: the replan, its verdict, and the
/// clean tree when there is one. The open change is not the plan's
/// concern; `commit` carries it.
pub(crate) fn plan(
    repo: &gix::Repository,
    branch: &str,
    onto: &Onto,
    message: Option<&str>,
) -> Result<MergePlan> {
    let (tip, already_in, replan) = replan_onto(repo, branch, onto)?;
    let message = crate::close::normalize_message(
        &message
            .map(String::from)
            .unwrap_or_else(|| default_message(onto, branch)),
    );
    let (conflict, tree) = if already_in {
        (None, None)
    } else {
        let conflict = rewrite::conflict(repo, replan.target, replan.tip, &replan.change)?;
        let tree = match &conflict {
            None => {
                Some(rewrite::chain(repo, replan.target, replan.tip, &replan.change, &[])?.tree)
            }
            Some(_) => None,
        };
        (conflict, tree)
    };
    Ok(MergePlan {
        branch: branch.to_string(),
        tip,
        onto: Onto {
            full: onto.full.clone(),
            name: onto.name.clone(),
            tip: onto.tip,
        },
        message,
        already_in,
        replan,
        conflict,
        tree,
    })
}

/// How the open change reaches the merge.
pub(crate) enum Bring<'a> {
    /// HEAD is on the branch: the open commit the capture wrote rides the
    /// merge through the arrival machinery, the way `ff switch` brings a
    /// park home. `held` is the standing merge hold this landing clears,
    /// when one stood.
    Open {
        head: &'a HeadState,
        held: Option<Box<HeldTransition>>,
    },
    /// A resolution landing: HEAD is on the session, and the return trip
    /// planned by `ff done` brings the branch's park home and clears the
    /// hold and the session.
    Resolution(&'a rewrite::Clearing),
}

/// Write the merge as one operation, verb `merge`: the commit, the branch
/// moved onto it in one CAS transaction, the index and the working copy
/// through the arrival, and the hold and session transitions the landing
/// clears. `tree` is the merge's tree: the plan's clean tree, or the
/// reader's fix.
pub(crate) fn commit(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: Vec<String>,
    plan: &MergePlan,
    tree: gix::ObjectId,
    bring: Bring<'_>,
) -> Result<MergeReport> {
    let now = ctx.now;
    let branch = plan.branch.clone();
    let branch_ref = format!("refs/heads/{branch}");
    let onto = &plan.onto;

    // The signer is resolved above every write: a bad `gpg.format` or a
    // missing key costs a config read and nothing else.
    let signer = sign::resolve(repo, sign::Choice::Config)?;
    let change_id = changeid::ChangeId::mint()?;
    let sig = refs::user_signature(repo, now)?;
    let merge = sign::write_user_commit(
        repo,
        signer.as_ref(),
        gix::objs::Commit {
            tree,
            parents: vec![plan.tip, onto.tip].into(),
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: plan.message.clone().into(),
            extra_headers: vec![changeid::header(&change_id)],
        },
    )?;

    // Write-ahead: the planned table is the post-merge world.
    let mut planned = observe_refs(repo)?;
    planned.refs.insert(branch_ref.clone(), merge.to_string());
    let mut record = OpRecord::new("merge", format!("merge {} into {branch}", onto.name), now);
    record.argv = argv;
    record.refs = vec![RefTransition {
        name: branch_ref.clone(),
        old: Some(plan.tip.to_string()),
        new: Some(merge.to_string()),
    }];
    let mut pins = vec![merge, plan.tip, onto.tip];
    pins.extend(ctx.pre_op.map(|id| id.object_id()));
    let mut stash_lines = stash::lines(repo)?;

    // The open change, or the return trip. Either folds its own arrival
    // into the record; the return trip folds the HEAD move and the
    // session's deletion with it.
    let return_trip = match &bring {
        Bring::Resolution(clearing) => clearing.return_trip.as_ref(),
        Bring::Open { .. } => None,
    };
    let mut hold_transition = None;
    let arrive = match &bring {
        Bring::Resolution(clearing) => {
            let (held, resolving) = held::clearing_transitions(clearing);
            record.held = held;
            record.resolving = resolving;
            let ret = return_trip
                .ok_or_else(|| Error::msg("internal: a resolution landing has no return trip"))?;
            let arrive = ret.arrival(repo, merge, tree, now)?;
            ret.fold_into(
                &mut planned,
                &mut record,
                &mut pins,
                &mut stash_lines,
                &arrive,
            );
            arrive
        }
        Bring::Open { head, held } => {
            record.held = held.as_deref().cloned();
            hold_transition = held.as_deref().cloned();
            let arrive = match crate::switch::park_of(repo, head, &branch, ctx.pre_tree)? {
                Some(open) => park::plan_replay(repo, &branch, open, merge, tree, now)?,
                None => ArrivePlan::none(),
            };
            arrive.fold_into(&mut planned, &mut record, &mut pins, &mut stash_lines);
            arrive
        }
    };
    let (end_tree, end_index) = arrive.end_trees(tree);
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: end_index,
            branch: match return_trip {
                Some(ret) => ret.to.clone(),
                None => branch.clone(),
            },
            base: Some(plan.tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        arrive.open_hint(),
        now,
    )?;

    // Mutate. HEAD leaves the session, whose branch the transaction deletes;
    // then the refs, all or none.
    if let Some(ret) = return_trip {
        ret.leave(repo, now)?;
    }
    let mut edits = vec![refs::update_edit(
        &branch_ref,
        merge,
        gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
            plan.tip,
        )),
        &format!("merge: {} into {branch}", onto.name),
    )?];
    if let Some(ret) = return_trip {
        edits.extend(ret.edits()?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                format!("{branch} moved while merging; nothing was merged (re-run ff resolve)"),
                vec![],
            ));
        }
    }

    // The hold this landing clears, before the arrival, which may record
    // one of its own.
    if let Some(t) = &hold_transition {
        held::set(repo, &branch, t.new.clone())?;
    }

    // Index and working copy: the return trip's, from the session's fixes
    // to the merge with the park brought home; or from the tree underfoot
    // to the merge, with the open change laid back over it.
    let everything = |_: &str| true;
    let (arrival, files) = match return_trip {
        Some(ret) => ret.land(repo, tree, &arrive, now)?,
        None => {
            crate::index::write_index_for_tree(repo, tree)?;
            let transition =
                crate::worktree::apply_tree_transition(repo, ctx.pre_tree, tree, &everything)?;
            let files = transition.written.len() + transition.deleted.len();
            let arrival = park::execute_arrival(repo, &branch, &arrive, tree, now)?;
            (arrival, files)
        }
    };
    let _ = futures::cache::remove(repo, &branch);

    Ok(MergeReport {
        branch,
        target: onto.name.clone(),
        commit: merge.to_string(),
        parents: [plan.tip.to_string(), onto.tip.to_string()],
        files,
        arrival,
    })
}

/// Land a resolved merge: the plan rebuilt from the hold's intent, the tree
/// the reader fixed — keyed by the base's tip, the one step's `old` — and
/// the commit written with the clearing `ff done` planned.
pub(crate) fn land_resolution(
    repo: &gix::Repository,
    rec: &held::Recording<'_>,
    hold: &Held,
    decided: &rewrite::Decided,
) -> Result<(MergeReport, ArrivalReport)> {
    let held::Intent::Merge {
        branch,
        onto,
        message,
    } = &hold.intent
    else {
        return Err(Error::msg("internal: land_resolution takes a held merge"));
    };
    let onto = restack::resolve_onto(repo, onto)?;
    let plan = plan(repo, branch, &onto, message.as_deref())?;
    let tree = decided.trees.get(&onto.tip).copied().ok_or_else(|| {
        Error::msg(format!(
            "internal: the decided merge of {} carries no tree for it",
            onto.name
        ))
    })?;
    let clearing = decided
        .clearing
        .as_ref()
        .ok_or_else(|| Error::msg("internal: a resolution landing carries its clearing"))?;
    let report = commit(
        repo,
        rec.ctx,
        rec.prov,
        rec.argv.clone(),
        &plan,
        tree,
        Bring::Resolution(clearing),
    )?;
    let arrival = report.arrival.clone();
    Ok((report, arrival))
}
