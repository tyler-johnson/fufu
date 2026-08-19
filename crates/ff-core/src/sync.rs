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
//! > operation log accounts for is yours. Anything else is theirs.
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
//! The network is somebody else's job. The tracking ref's tip before and
//! after the fetch is handed in as a parameter — that is what keeps this
//! whole module testable without a git binary.

use crate::model::{BaseAxis, Pending, RemoteAxis, RestackOutcome, SyncReport};
use crate::preflight::Preflight;
use crate::restack::restack;
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
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

pub fn sync(
    repo: &gix::Repository,
    pre: &Preflight,
    opts: SyncOptions,
    prov: &Provenance,
) -> Result<(SyncReport, crate::ops::verb::VerbContext)> {
    let ctx = crate::ops::verb::begin_verb(repo, prov, opts.now)?;

    // The remote axis: the shared copy of this same branch. `restack`
    // decides up-to-date versus fast-forward versus replay from the same
    // merge bases — the only thing this axis decides is whether to call it.
    let remote = match pre.tracking.as_ref() {
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
            // Divergence this run's fetch did not bring in is yours only if
            // the operation log accounts for every commit the remote holds
            // and you do not; anything unaccounted for falls to replay.
            let yours = if diverged && !arrived {
                let behind = crate::upstream::exclusive(repo, after, &bases)?;
                let hexes: Vec<String> = behind.iter().map(|id| id.to_string()).collect();
                let accounted = crate::accounted_for(repo, &hexes)?;
                (accounted.len() == hexes.len()).then_some(behind.len())
            } else {
                None
            };
            if let Some(behind) = yours {
                let ahead = crate::upstream::count_exclusive(repo, pre.branch_tip, &bases)?;
                RemoteAxis::Yours {
                    name: tracking.name.clone(),
                    ahead,
                    behind,
                }
            } else {
                let (outcome, _) = restack(
                    repo,
                    None,
                    Some(tracking.full.clone()),
                    prov,
                    Some(ctx.now),
                    opts.argv.clone(),
                )?;
                RemoteAxis::Ran {
                    name: tracking.name.clone(),
                    outcome,
                }
            }
        }
    };

    // The base axis, read AFTER the remote axis has run: that one may have
    // moved the branch, and a base computed against the old tip would be
    // answering a question nobody asked. `onto: None` keeps it from
    // recording a parent it did not choose.
    let base = match &remote {
        RemoteAxis::Ran {
            outcome: RestackOutcome::Held(_),
            ..
        } => BaseAxis::Skipped,
        _ => match crate::futures::base_for(repo, &pre.branch)? {
            None => BaseAxis::NoBase,
            Some(sync_ref) => {
                let (outcome, _) =
                    restack(repo, None, None, prov, Some(ctx.now), opts.argv.clone())?;
                BaseAxis::Ran {
                    name: sync_ref.name,
                    outcome,
                }
            }
        },
    };

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
            Pending::Ahead(crate::upstream::count_exclusive(repo, tip_now, &bases)?)
        }
    };

    Ok((
        SyncReport {
            branch: pre.branch.clone(),
            fetched: opts.fetched && pre.remote.is_some(),
            remote,
            base,
            pending,
        },
        ctx,
    ))
}
