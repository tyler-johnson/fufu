//! `ff sync`: the incoming half of lining up.
//!
//! Sync takes in. `ff publish` sends out. They were one verb once, and the
//! split is the point: everything sync does is undoable, and publishing —
//! the one act that leaves the machine and cannot be taken back — is a
//! verb you type on purpose.
//!
//! A branch answers to two things — the **base** beneath it and the
//! **remote** copy of itself — and reconciling with either is a replay, so
//! both axes are `restack` calls. This module mostly decides *whether to
//! call* `restack`; `restack` decides the rest.
//!
//! The one decision that is sync's alone is whose divergence it is. After
//! any restack your local branch diverges from `origin/<branch>` — so does
//! a branch a collaborator pushed to. They are the same shape and the
//! correct answers are opposite: one wants a force-push, the other a replay
//! onto the remote, and getting it backwards either replays your rebased
//! commits onto their stale originals, silently undoing the restack, or
//! force-pushes over somebody else's work. The rule is:
//!
//! > Divergence this run's fetch created is theirs. Divergence your own
//! > operation log accounts for is yours. A tracking tip standing exactly
//! > where this branch last published it is yours too, and undone. Anything
//! > else is theirs.
//!
//! The first clause is free and certain: the fetch moved the tracking ref,
//! so someone else wrote what arrived, the axis is **incoming**, and sync
//! replays onto the new remote tip. The second is a lookup rather than an
//! assumption — every commit the remote has and you do not must appear in
//! the log as the `old` side of a rewrite, or as one a replay dropped as
//! empty. Only then is the axis **outgoing**, with nothing to take in and
//! the publish left to handle it.
//!
//! The third clause is why the second is checked. Their commits reach the
//! tracking ref through *any* fetch: an editor's background one, a manual
//! `git fetch`, an earlier sync that fetched and then held on a conflict.
//! This run's fetch then finds nothing new, and reading that silence as
//! "the divergence is mine" force-pushes over their work under a lease that
//! cannot catch it — the lease expects the tip the remote already holds, so
//! it holds. Unrecognized means replay, which never loses work.
//! `--no-fetch` needs no rule of its own: it reaches the same check.
//!
//! The undone clause is the one case the third clause used to swallow. An
//! unmoved tracking ref is silence *unless the log says who put it there*,
//! and since publish records its pushes it does: a tracking tip equal to the
//! newest publish row's `to` is a tip this repository sent, so everything
//! reachable from it that you now lack was yours when you sent it. Undoing
//! the commit that made it does not reach across the wire, and replaying it
//! back in would reverse the undo and call it arriving work. The publish is
//! what rolls the shared copy back. This clause runs *after* the accounted
//! one, which satisfies both when you publish and then rewrite — and there
//! "stale copies of your own" is the truer sentence.
//!
//! The network is somebody else's job. The tracking ref's tip before and
//! after the fetch is handed in as a parameter — that is what keeps this
//! whole module testable without a git binary.
//!
//! A sync is one operation, whatever moved. Every axis of every branch is
//! planned through `restack`'s planning half against one overlay of what
//! the run has already decided, and the run is written once: the record
//! ahead of any ref move, the refs in one transaction, the holds onto their
//! branches, and the worktree last. One `ff undo` takes the whole run back.

use std::collections::HashSet;

use crate::model::{
    BaseAxis, BranchRemote, BranchSync, Pending, RemoteAxis, RestackOutcome, SkipReason, SyncReport,
};
use crate::ops::record::{HeldTransition, RefTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, verb};
use crate::overlay::Overlay;
use crate::preflight::Preflight;
use crate::refs;
use crate::restack::{Aim, RestackPlan, plan_restack};
use crate::{Error, Provenance, Result};

/// The facts sync cannot learn for itself, handed in by whoever ran the
/// network. Parameters rather than a fetch here is the whole reason this file
/// is testable without a git binary.
pub struct SyncOptions {
    /// A fetch ran this invocation.
    pub fetched: bool,
    /// The tracking ref's tip after the fetch — equal to `Tracking::tip` when
    /// nothing arrived, `None` when the ref is still absent.
    pub tracking_after: Option<gix::ObjectId>,
    /// Every local branch other than the one underfoot, with its tracking
    /// ref read on both sides of the fetch. [`other_branches`] reads them
    /// and [`after_fetch`] carries the second reading into the first.
    pub others: Vec<OtherBranch>,
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// A local branch other than the one underfoot, as the remote axis needs
/// it: its tip and, when it has an upstream, the tracking ref on both sides
/// of the fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherBranch {
    pub branch: String,
    pub tip: gix::ObjectId,
    pub tracking: Option<OtherTracking>,
}

/// The shared copy of a branch not underfoot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherTracking {
    /// The full ref: `refs/remotes/origin/side`.
    pub full: String,
    /// What a person calls it: `origin/side`.
    pub name: String,
    /// The remote the tracking ref belongs to: `origin`. The axis acts only
    /// when this is the remote the run fetched from.
    pub remote: String,
    /// Its tip before the fetch. `None` when the ref is absent.
    pub before: Option<gix::ObjectId>,
    /// Its tip after the fetch. `None` when the ref is absent, or when no
    /// second reading was taken.
    pub after: Option<gix::ObjectId>,
}

/// Every local branch other than `current`, with its tracking ref as it
/// stands now filled into `before` and `after` left empty. Called twice
/// around the fetch; [`after_fetch`] joins the two readings.
pub fn other_branches(repo: &gix::Repository, current: &str) -> Result<Vec<OtherBranch>> {
    let mut out = Vec::new();
    for branch in crate::switch::branch_names(repo)? {
        if branch == current {
            continue;
        }
        let tip = crate::preflight::branch_tip(repo, &branch)?;
        let tracking = match crate::futures::remote_for(repo, &branch)? {
            None => None,
            Some(sync_ref) => {
                let before = (!sync_ref.tip.is_empty())
                    .then(|| gix::ObjectId::from_hex(sync_ref.tip.as_bytes()))
                    .transpose()
                    .map_err(Error::repo)?;
                // `branch.<n>.remote` names the remote; the ref's second
                // segment is the fallback for an upstream configured some
                // other way.
                let remote = repo
                    .branch_remote_name(branch.as_str(), gix::remote::Direction::Fetch)
                    .as_ref()
                    .and_then(|name| name.as_symbol())
                    .map(|name| name.to_string())
                    .or_else(|| {
                        sync_ref
                            .r#ref
                            .strip_prefix("refs/remotes/")
                            .and_then(|rest| rest.split('/').next())
                            .map(|remote| remote.to_string())
                    })
                    .unwrap_or_default();
                Some(OtherTracking {
                    full: sync_ref.r#ref,
                    name: sync_ref.name,
                    remote,
                    before,
                    after: None,
                })
            }
        };
        out.push(OtherBranch {
            branch,
            tip,
            tracking,
        });
    }
    Ok(out)
}

/// The pre-fetch reading with each branch's post-fetch tracking tip copied
/// into `after`, matched by branch name. A branch the second reading does
/// not list, or lists without an upstream, keeps `after: None`.
pub fn after_fetch(mut before: Vec<OtherBranch>, after: &[OtherBranch]) -> Vec<OtherBranch> {
    for other in &mut before {
        if let Some(tracking) = other.tracking.as_mut() {
            tracking.after = after
                .iter()
                .find(|o| o.branch == other.branch)
                .and_then(|o| o.tracking.as_ref())
                .and_then(|t| t.before);
        }
    }
    before
}

pub fn sync(
    repo: &gix::Repository,
    pre: &Preflight,
    opts: SyncOptions,
    prov: &Provenance,
) -> Result<(SyncReport, crate::ops::verb::VerbContext)> {
    let ctx = crate::ops::verb::begin_verb(repo, prov, opts.now)?;
    let mut run = Run::new(&pre.branch, ctx.now);

    // Two phases over the whole repository. The remote phase first, for
    // every branch: each branch against its own shared copy. Then the base
    // phase, parent before child: each branch against the base beneath it.
    // The remote phase finishes everywhere before the base phase starts,
    // because the divergence rule reads the operation log through
    // `accounted_for`, and a base-axis replay recorded earlier in the run
    // would answer for a commit it should not.
    //
    // Nothing is written until both phases are planned. Every restack is
    // planned against the run's overlay, which holds the tips, holds, and
    // worktree the run has decided so far, and the whole run is committed
    // once at the end as one `sync` operation.
    //
    // The remote axis of the branch underfoot: the shared copy of this same
    // branch. `restack` decides up-to-date versus fast-forward versus replay
    // from the same merge bases — the only thing this axis decides is
    // whether to call it.
    let mut remote = match pre.tracking.as_ref() {
        None => RemoteAxis::NoRemote,
        Some(tracking) if opts.tracking_after.is_none() => RemoteAxis::Gone {
            name: tracking.name.clone(),
        },
        Some(tracking) => {
            let after = opts.tracking_after.expect("checked above");
            let bases: Vec<gix::ObjectId> = repo
                .merge_bases_many(pre.branch_tip, &[after])
                .map_err(Error::repo)?
                .into_iter()
                .map(|id| id.detach())
                .collect();
            let diverged = !bases.contains(&after) && !bases.contains(&pre.branch_tip);
            let arrived = opts.fetched && opts.tracking_after != tracking.tip;
            // What the remote holds and this branch does not, walked once:
            // both outgoing answers are measured in exactly these commits.
            let theirs = crate::upstream::exclusive(repo, after, &bases)?;
            let behind = theirs.len();
            // Divergence this run's fetch did not bring in is yours only if
            // the operation log accounts for every commit the remote holds
            // and you do not; anything unaccounted for falls to replay.
            let yours = if diverged && !arrived {
                let hexes: Vec<String> = theirs.iter().map(|id| id.to_string()).collect();
                let accounted = crate::accounted_for(repo, &hexes)?;
                (accounted.len() == hexes.len()).then_some(behind)
            } else {
                None
            };
            // The tip the log says we last sent. Nothing arrived this run and
            // the remote has not moved off it, so what it holds that we lack
            // is work of ours we stepped back from — including the strict
            // ancestor case, where there is no divergence to account for at
            // all and the plain fast-forward would take the undo straight
            // back in.
            let undone = yours.is_none()
                && !arrived
                && behind > 0
                && crate::published::published_tip(repo, &pre.branch, &after.to_string())?;
            if let Some(behind) = yours {
                let ahead = crate::upstream::count_exclusive(repo, pre.branch_tip, &bases)?;
                RemoteAxis::Yours {
                    name: tracking.name.clone(),
                    ahead,
                    behind,
                }
            } else if undone {
                RemoteAxis::Undone {
                    name: tracking.name.clone(),
                    behind,
                }
            } else {
                let plan = plan_restack(
                    repo,
                    None,
                    Some(tracking.full.clone()),
                    ctx.now,
                    &crate::rewrite::Decided::none(),
                    Aim::Settled,
                    &run.overlay,
                )?;
                RemoteAxis::Ran {
                    name: tracking.name.clone(),
                    outcome: run.fold(repo, plan)?,
                }
            }
        }
    };

    // The remote axis of every other local branch. A fast-forward and a
    // mirror move are ref-only; a divergence gets the same rule as the
    // branch underfoot: yours is named and left standing, theirs replays
    // through `restack`'s planning half, which moves refs and objects and no
    // file for a branch nobody is standing on. A hold the run already
    // planned on a branch, by the branch underfoot's cascade, files it as
    // held the way a hold from an earlier run is. The base axis of every
    // row is filled in by the base phase.
    let holders = crate::linked::holders(repo)?;
    let mut branches = Vec::with_capacity(opts.others.len());
    for other in &opts.others {
        let branch = other.branch.clone();
        if let Some(holder) = holders.iter().find(|h| h.branch == branch) {
            branches.push(BranchSync::Elsewhere {
                branch,
                path: holder.path.display().to_string(),
            });
            continue;
        }
        if let Some(held) = run.overlay.held(repo, &branch)? {
            branches.push(BranchSync::Held {
                branch,
                verb: crate::held::verb_of(&held),
            });
            continue;
        }
        let remote = other_remote_axis(repo, other, pre, &opts, &mut run)?;
        branches.push(BranchSync::Synced {
            branch,
            remote,
            base: Box::new(BaseAxis::NoBase),
        });
    }

    // The base phase, parent first, over the branch underfoot and every
    // branch with a row. Read AFTER the remote phase has finished
    // everywhere: that phase may have moved any of these branches, and a
    // base computed against an old tip would be answering a question nobody
    // asked. `onto: None` keeps each restack from recording a parent it did
    // not choose. A restack cascades into the branches stacked above it, so
    // a child reached later in the order finds itself already on its moved
    // parent and reads up to date: that is the correct report, and nothing
    // replays twice. A branch another worktree holds kept its `Elsewhere`
    // row and has no base axis, and neither does one filed as `Held`.
    let mut base = BaseAxis::NoBase;
    for name in base_order(repo)? {
        if name == pre.branch {
            base = current_base_axis(repo, pre, &mut run)?;
            continue;
        }
        let row = branches.iter_mut().find(|row| match row {
            BranchSync::Synced { branch, .. } => *branch == name,
            _ => false,
        });
        if let Some(BranchSync::Synced { base, .. }) = row {
            **base = other_base_axis(repo, &name, &mut run)?;
        }
    }

    // The one operation: everything both phases planned, written ahead of
    // the first ref move, then the refs in one transaction, then the holds,
    // then the worktree. A run that planned nothing records nothing.
    let (files, still_open) = if run.is_empty() {
        (0, false)
    } else {
        run.commit(repo, &ctx, pre, prov, opts.argv.clone())?
    };
    // The files the one worktree write touched are the run's, since the
    // branch underfoot may have been carried by another branch's cascade
    // and then have no landed axis of its own. When one of its axes did
    // land, that report says the same number: the base axis when it landed,
    // since it ran last, and the remote axis otherwise.
    if files > 0 {
        let base_landed = match &mut base {
            BaseAxis::Ran {
                outcome: RestackOutcome::Restacked(r),
                ..
            } => Some(r),
            _ => None,
        };
        let remote_landed = match &mut remote {
            RemoteAxis::Ran {
                outcome: RestackOutcome::Restacked(r),
                ..
            } => Some(r),
            _ => None,
        };
        if let Some(r) = base_landed.or(remote_landed) {
            r.files = files;
        }
    }

    // What is left waiting. The tip is read again because both axes may have
    // moved it, and the count is against the tracking ref as it now stands —
    // the same fact `ff publish` will lease against. Sync names it and sends
    // nothing.
    let pending = match (pre.remote.as_ref(), opts.tracking_after) {
        (None, _) => Pending::NoRemote,
        // A remote is configured but this branch has no copy on it — either
        // it never had one or somebody deleted it. Both are publish's to fix,
        // and neither is a number.
        (Some(_), None) => Pending::Unpublished,
        (Some(_), Some(after)) => {
            let tip_now = crate::preflight::branch_tip(repo, &pre.branch)?;
            let bases: Vec<gix::ObjectId> = repo
                .merge_bases_many(tip_now, &[after])
                .map_err(Error::repo)?
                .into_iter()
                .map(|id| id.detach())
                .collect();
            // An undone publish is not a count of commits to send — it is a
            // count still out there to take off, and the same verb clears it.
            // Measured again here because the base axis may have moved the
            // tip since the remote axis decided.
            if matches!(remote, RemoteAxis::Undone { .. }) {
                Pending::Undone(crate::upstream::count_exclusive(repo, after, &bases)?)
            } else {
                Pending::Ahead(crate::upstream::count_exclusive(repo, tip_now, &bases)?)
            }
        }
    };

    Ok((
        SyncReport {
            branch: pre.branch.clone(),
            fetched: opts.fetched && pre.remote.is_some(),
            remote,
            base,
            branches,
            files,
            still_open,
            pending,
        },
        ctx,
    ))
}

/// Everything one run has planned and not written: the overlay the planners
/// read, and the pieces of the one `sync` operation. A restack's plan is
/// folded in as it is made, so the next plan reads the tips, holds, and
/// worktree the last one decided.
struct Run {
    /// The branch underfoot: a hold on it is the record's own `held`, and
    /// every other hold rides `cascade_held`, the list undo reads beside it.
    branch: String,
    now: i64,
    overlay: Overlay,
    /// Every transition in the order it was planned; one ref may appear more
    /// than once, and [`coalesce`] folds those for the record and the
    /// transaction.
    refs: Vec<RefTransition>,
    rewrites: Vec<crate::rewrite::Rewrite>,
    dropped: Vec<crate::rewrite::Dropped>,
    held: Option<HeldTransition>,
    cascade_held: Vec<HeldTransition>,
    pins: Vec<gix::ObjectId>,
}

impl Run {
    fn new(branch: &str, now: i64) -> Self {
        Run {
            branch: branch.to_string(),
            now,
            overlay: Overlay::default(),
            refs: Vec::new(),
            rewrites: Vec::new(),
            dropped: Vec::new(),
            held: None,
            cascade_held: Vec::new(),
            pins: Vec::new(),
        }
    }

    /// Nothing planned: no ref moves and no hold, so no operation.
    fn is_empty(&self) -> bool {
        self.refs.is_empty() && self.held.is_none() && self.cascade_held.is_empty()
    }

    /// A ref-only move: a fast-forward or a mirror move onto the shared copy.
    fn moved(&mut self, t: RefTransition) -> Result<()> {
        if let Some(new) = &t.new {
            let id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
            self.overlay.set_tip(&t.name, id);
            self.pins.push(id);
        }
        self.refs.push(t);
        Ok(())
    }

    /// Fold a planned restack in: what it moves onto the overlay, its
    /// record pieces onto the run, and the outcome the axis reports. A
    /// replay's report is built now, with no files written, since the one
    /// worktree write comes at the end of the run.
    fn fold(&mut self, repo: &gix::Repository, plan: RestackPlan) -> Result<RestackOutcome> {
        match plan {
            RestackPlan::Unchanged { branch, base } => {
                Ok(RestackOutcome::NothingToRestack { branch, base })
            }
            RestackPlan::Held(plan) => {
                let transition = HeldTransition {
                    branch: plan.branch.clone(),
                    old: None,
                    new: Some(plan.held.clone()),
                };
                if plan.branch == self.branch {
                    self.held = Some(transition);
                } else {
                    self.cascade_held.push(transition);
                }
                self.overlay.hold(&plan.branch, plan.held.clone());
                Ok(RestackOutcome::Held(plan.report.clone()))
            }
            RestackPlan::Replay(plan) => {
                let report = plan.report(repo, 0, None)?;
                for t in plan.carried.iter().chain(plan.cascade.carried.iter()) {
                    if let Some(new) = &t.new {
                        let id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
                        self.overlay.set_tip(&t.name, id);
                    }
                    self.refs.push(t.clone());
                }
                self.rewrites.extend(plan.rewrites.iter().cloned());
                self.rewrites.extend(plan.cascade.rewrites.iter().cloned());
                self.dropped.extend(plan.dropped.iter().cloned());
                self.dropped.extend(plan.cascade.dropped.iter().cloned());
                for h in &plan.cascade.holds {
                    if let Some(held) = &h.new {
                        self.overlay.hold(&h.branch, held.clone());
                    }
                    self.cascade_held.push(h.clone());
                }
                self.pins.extend(plan.own_pins()?);
                self.pins.extend(plan.cascade.pins.iter().copied());
                if let (Some(open), Some(tip), Some(worktree)) =
                    (plan.open, plan.new_head_tip, plan.new_worktree)
                {
                    self.overlay.move_head(open, tip, worktree);
                }
                Ok(RestackOutcome::Restacked(Box::new(report)))
            }
        }
    }

    /// The one operation: the record write-ahead, the refs in one
    /// transaction, the holds onto their branches, the worktree last. The
    /// files the worktree write touched come back, with whether the tree
    /// still holds an open change against the tip it now sits on.
    fn commit(
        self,
        repo: &gix::Repository,
        ctx: &verb::VerbContext,
        pre: &Preflight,
        prov: &Provenance,
        argv: Vec<String>,
    ) -> Result<(usize, bool)> {
        let Run {
            overlay,
            refs,
            rewrites,
            dropped,
            held,
            cascade_held,
            pins,
            ..
        } = self;
        let refs = coalesce(refs);
        let mut planned = observe_refs(repo)?;
        for t in &refs {
            if let Some(new) = &t.new {
                planned.refs.insert(t.name.clone(), new.clone());
            }
        }
        let holds = held.iter().count() + cascade_held.len();
        let summary = match holds {
            0 => format!("sync {} branch(es)", refs.len()),
            n => format!("sync {} branch(es), {n} held", refs.len()),
        };
        let mut record = OpRecord::new("sync", summary, ctx.now);
        record.argv = argv;
        record.refs = refs.clone();
        record.rewrites = rewrites;
        record.dropped = dropped;
        record.held = held.clone();
        record.cascade_held = cascade_held.clone();

        // The trees the op leaves behind: the worktree the run planned when
        // it moved the branch underfoot, and the ones it found otherwise.
        let (tree, index_tree) = match &overlay.head {
            Some(head) => (head.worktree, crate::futures::tree_of(repo, head.tip)?),
            None => (ctx.pre_tree, crate::index::tree_from_index(repo)?),
        };
        verb::append_op(
            repo,
            OpKind::Op,
            verb::VerbOp {
                record,
                planned,
                tree,
                index_tree,
                branch: pre.branch.clone(),
                base: Some(pre.branch_tip),
                session: prov.session.clone(),
                pins: &pins,
            },
            ctx.now,
        )?;

        // The refs: every branch the run moved, in one transaction, each
        // expected where the run found it.
        let reflog_msg = "sync";
        let mut edits = Vec::with_capacity(refs.len());
        for t in &refs {
            let (Some(old), Some(new)) = (&t.old, &t.new) else {
                continue;
            };
            let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
            let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
            edits.push(refs::update_edit(
                &t.name,
                new_id,
                gix::refs::transaction::PreviousValue::MustExistAndMatch(
                    gix::refs::Target::Object(old_id),
                ),
                reflog_msg,
            )?);
        }
        match refs::commit_edits(repo, edits, ctx.now)? {
            refs::EditOutcome::Applied => {}
            refs::EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    "refs moved while syncing; nothing was moved (re-run to sync on the new tips)",
                    vec![],
                ));
            }
        }

        // The holds, now that the refs have moved.
        for h in held.iter().chain(cascade_held.iter()) {
            crate::held::set(repo, &h.branch, h.new.clone())?;
        }

        // The worktree, last: index first, then the files, the order
        // switch.rs uses, from the open change as the run found it to the
        // tree the last move planned.
        let mut files = 0usize;
        let mut still_open = false;
        if let Some(head) = &overlay.head {
            let tip_tree = crate::futures::tree_of(repo, head.tip)?;
            crate::index::write_index_for_tree(repo, tip_tree)?;
            let everything = |_: &str| true;
            let transition = crate::worktree::apply_tree_transition(
                repo,
                head.open,
                head.worktree,
                &everything,
            )?;
            files = transition.written.len() + transition.deleted.len();
            // What is still open is the planned worktree against the tip it
            // now sits on, the measure `restack`'s report takes.
            still_open = head.worktree != tip_tree;
        }

        // Futures caches of every branch that moved. Best-effort: it costs
        // recomputation and nothing else.
        for t in &refs {
            if let Some(name) = t.name.strip_prefix("refs/heads/") {
                let _ = crate::futures::cache::remove(repo, name);
            }
        }
        Ok((files, still_open))
    }
}

/// One transition per ref, from where the run found it to where it left
/// it: a branch its remote axis fast-forwarded and its base axis then
/// replayed moved twice in the plan and once in the world.
fn coalesce(refs: Vec<RefTransition>) -> Vec<RefTransition> {
    let mut out: Vec<RefTransition> = Vec::with_capacity(refs.len());
    for t in refs {
        match out.iter_mut().find(|o| o.name == t.name) {
            Some(o) => o.new = t.new,
            None => out.push(t),
        }
    }
    out
}

/// The remote axis of one branch not underfoot. Decides, and when the
/// branch follows its shared copy folds the ref move into the run. A
/// divergence that is theirs is planned through `restack`, which writes
/// commit objects and no ref and no file, and its hold or its replay is
/// folded in the same way; nothing is written here.
fn other_remote_axis(
    repo: &gix::Repository,
    other: &OtherBranch,
    pre: &Preflight,
    opts: &SyncOptions,
    run: &mut Run,
) -> Result<BranchRemote> {
    let Some(tracking) = other.tracking.as_ref() else {
        return Ok(BranchRemote::NoRemote);
    };
    let name = tracking.name.clone();
    // A tip this run did not fetch is one it cannot trust: the wrong remote,
    // no remote, or `--no-fetch`.
    if !opts.fetched || pre.remote.as_deref() != Some(tracking.remote.as_str()) {
        return Ok(BranchRemote::NotFetched { name });
    }
    let Some(after) = tracking.after else {
        return Ok(BranchRemote::Gone { name });
    };
    // The tip as the run has planned it rather than the pre-run reading: the
    // branch underfoot's remote axis ran first and its cascade may have
    // moved this one, and a decision made against the old tip would hand
    // the ref transaction a value the ref no longer holds. A branch a
    // cascade moved no longer stands where the tracking ref stood, so it is
    // not mirror-moved; it falls to the divergence rule like any other.
    let tip = run
        .overlay
        .branch_tip(repo, &other.branch)?
        .unwrap_or(other.tip);
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(tip, &[after])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    // The shared copy holds nothing this branch lacks: level, or the branch
    // is ahead of it. Either way there is nothing to take in, which is the
    // same answer `restack` gives the branch underfoot.
    if tip == after || bases.contains(&after) {
        return Ok(BranchRemote::UpToDate { name });
    }
    let arrived = tracking.after != tracking.before;
    // What the remote holds and this branch does not, walked once: the
    // undone guard and the accounted-for check are both measured in
    // exactly these commits.
    let theirs = crate::upstream::exclusive(repo, after, &bases)?;
    let behind = theirs.len();
    // The undone guard, before any move: an unmoved tracking tip standing
    // where this repository last published the branch holds work of yours
    // you stepped back from, and a fast-forward would take the undo
    // straight back in.
    if !arrived
        && behind > 0
        && crate::published::published_tip(repo, &other.branch, &after.to_string())?
    {
        return Ok(BranchRemote::Undone { name, behind });
    }
    let fast_forward = bases.contains(&tip);
    // Mirror-moved: the branch stood exactly where the tracking ref stood
    // before the fetch, so it follows wherever the remote went, force-push
    // included. Otherwise a plain fast-forward is the one ref-only move
    // left.
    if (tracking.before == Some(tip) && arrived) || fast_forward {
        run.moved(RefTransition {
            name: format!("refs/heads/{}", other.branch),
            old: Some(tip.to_string()),
            new: Some(after.to_string()),
        })?;
        return Ok(BranchRemote::Moved {
            name,
            fast_forward,
            behind,
            old: tip.to_string(),
            new: after.to_string(),
        });
    }
    // A divergence, under the rule the module doc states. Divergence this
    // run's fetch created is theirs. Divergence it did not create is yours
    // only if the operation log accounts for every commit the remote holds
    // and this branch does not; anything unaccounted for falls to replay,
    // which never loses work.
    let yours = if !arrived {
        let hexes: Vec<String> = theirs.iter().map(|id| id.to_string()).collect();
        let accounted = crate::accounted_for(repo, &hexes)?;
        accounted.len() == hexes.len()
    } else {
        false
    };
    if yours {
        let ahead = crate::upstream::count_exclusive(repo, tip, &bases)?;
        return Ok(BranchRemote::Yours {
            name,
            ahead,
            behind,
        });
    }
    // Theirs: replay onto the new remote tip. Naming the branch is what
    // keeps the worktree out of it, and `Aim::Settled` is what lets the
    // branch aim at its own shared copy. A conflict holds on this branch,
    // planned here and recorded with the run, and the run goes on to the
    // next one: holds are per branch.
    let plan = plan_restack(
        repo,
        Some(other.branch.clone()),
        Some(tracking.full.clone()),
        run.now,
        &crate::rewrite::Decided::none(),
        Aim::Settled,
        &run.overlay,
    )?;
    Ok(BranchRemote::Ran {
        name,
        outcome: run.fold(repo, plan)?,
    })
}

/// The order the base phase visits local branches: every branch after the
/// one [`crate::futures::base_for`] answers for it, so a parent is replayed
/// before its child and the child finds its base already moved. A
/// depth-first walk from the roots, which are the branches with no base or
/// with a base that is not a local branch, such as `origin/main`, taking
/// children by name. A parent link `--onto` aimed in a loop has no root; its
/// members come after every rooted tree, in name order, each visited once.
pub fn base_order(repo: &gix::Repository) -> Result<Vec<String>> {
    let names = crate::switch::branch_names(repo)?;
    let stack = crate::cascade::Stack::read(repo)?;
    let filed: HashSet<&str> = names
        .iter()
        .flat_map(|name| stack.children(name).iter().map(String::as_str))
        .collect();
    let mut out = Vec::with_capacity(names.len());
    let mut seen: HashSet<&str> = HashSet::with_capacity(names.len());
    let roots = names.iter().filter(|name| !filed.contains(name.as_str()));
    for start in roots.chain(names.iter()) {
        let mut pending: Vec<&str> = vec![start.as_str()];
        while let Some(name) = pending.pop() {
            if !seen.insert(name) {
                continue;
            }
            // Pushed in reverse so the first child by name is visited first.
            pending.extend(stack.children(name).iter().rev().map(String::as_str));
            out.push(name.to_string());
        }
    }
    Ok(out)
}

/// The base axis of the branch underfoot: a plain `restack` with `onto:
/// None`, planned against the run. A hold the run planned on it stops it,
/// whether its own remote axis held or a cascade from another branch's
/// replay reached it; a hold that stood before the run is the preflight's
/// to refuse and is not second-guessed here.
fn current_base_axis(repo: &gix::Repository, pre: &Preflight, run: &mut Run) -> Result<BaseAxis> {
    if run.overlay.has_hold(&pre.branch) {
        return Ok(BaseAxis::Skipped);
    }
    let Some(sync_ref) = crate::futures::base_for(repo, &pre.branch)? else {
        return Ok(BaseAxis::NoBase);
    };
    let plan = plan_restack(
        repo,
        None,
        None,
        run.now,
        &crate::rewrite::Decided::none(),
        Aim::Asked,
        &run.overlay,
    )?;
    Ok(BaseAxis::Ran {
        name: sync_ref.name,
        outcome: run.fold(repo, plan)?,
    })
}

/// The base axis of a branch not underfoot: `restack` by name with `onto:
/// None`, planned against the run, which moves refs and objects and no file
/// and cascades into the branches stacked above it. A hold standing on the
/// branch stops it: its own remote axis held this run, or a cascade reached
/// it. A replay `restack` refuses before anything moves is named and left
/// standing rather than stopping the run: sync visits every branch, and one
/// branch's merge or orphan history is no reason to leave the rest stale.
fn other_base_axis(repo: &gix::Repository, branch: &str, run: &mut Run) -> Result<BaseAxis> {
    if run.overlay.held(repo, branch)?.is_some() {
        return Ok(BaseAxis::Skipped);
    }
    let Some(sync_ref) = crate::futures::base_for(repo, branch)? else {
        return Ok(BaseAxis::NoBase);
    };
    let name = sync_ref.name;
    let plan = plan_restack(
        repo,
        Some(branch.to_string()),
        None,
        run.now,
        &crate::rewrite::Decided::none(),
        Aim::Asked,
        &run.overlay,
    );
    match plan {
        Ok(plan) => Ok(BaseAxis::Ran {
            name,
            outcome: run.fold(repo, plan)?,
        }),
        Err(err) if err.id() == "restack/unrelated" => Ok(BaseAxis::Refused {
            name,
            reason: SkipReason::Unrelated,
        }),
        Err(err) if err.id() == "rewrite/merge-in-range" => Ok(BaseAxis::Refused {
            name,
            reason: SkipReason::MergeInRange,
        }),
        Err(err) => Err(err),
    }
}
