//! `ff sync`: `ff status`'s sync line with the verbs attached.
//!
//! A branch answers to two things — the **base** beneath it and the
//! **remote** copy of itself — and reconciling with either is a replay, so
//! both axes are `restack` calls. This module mostly decides *whether to
//! call* `restack`; `restack` decides the rest.
//!
//! The one decision that is sync's alone is whose divergence it is. After
//! any restack your local branch diverges from `origin/<branch>` — so does
//! a branch a collaborator pushed to. They are the same shape and the
//! correct answers are opposite: the first wants a force-push, the second
//! wants a replay onto the remote — and getting it backwards replays your
//! rebased commits back onto their stale originals, silently undoing the
//! restack. The rule is:
//!
//! > Divergence that this run's fetch created is theirs. Divergence that
//! > was already there is yours.
//!
//! If the fetch moved the tracking ref and the branch now diverges,
//! someone else wrote those commits: the axis is **incoming**, and sync
//! replays onto the new remote tip. If the fetch did not move it, the
//! divergence can only be a rewrite of your own: the axis is **outgoing**,
//! there is nothing to take in, and the publish is what handles it.
//! `--no-fetch` falls out for free: with no fetch nothing new arrived, so
//! nothing is theirs.
//!
//! The network is somebody else's job. The tracking ref's tip before and
//! after the fetch is handed in as a parameter — that is what keeps this
//! whole module testable without a git binary.

use crate::model::{BaseAxis, HeadState, Publish, RemoteAxis, RestackOutcome, SyncReport};
use crate::restack::restack;
use crate::{Error, Provenance, Result};

/// What `ff sync` must know before it can reach the network: the branch
/// underfoot, the remote it answers to, and where the tracking ref stood
/// before anything was fetched. Every guard that can refuse has already run
/// by the time this returns — a fetch is a round trip, and paying for one
/// only to refuse afterwards is the rudest possible order.
pub struct Preflight {
    pub branch: String,
    pub branch_tip: gix::ObjectId,
    /// The remote to fetch from and push to. `None` when the repository has
    /// no remote at all, which makes sync entirely a base-axis affair.
    pub remote: Option<String>,
    /// The branch's shared copy, when it has an upstream.
    pub tracking: Option<Tracking>,
    /// A rewrite is already held on this branch, so its exit is blocked
    /// before sync has done anything at all.
    pub held: bool,
}

pub struct Tracking {
    /// The full ref: `refs/remotes/origin/feature`.
    pub full: String,
    /// What a person calls it: `origin/feature`.
    pub name: String,
    /// The branch's name on the remote side, from `branch.<n>.merge`:
    /// `feature`. A lease and a refspec are written in terms of this and not
    /// of the local name, which is free to differ.
    pub remote_branch: String,
    /// Its tip as it stands, before any fetch. `None` when it is absent.
    pub tip: Option<gix::ObjectId>,
}

pub fn preflight(repo: &gix::Repository) -> Result<Preflight> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to sync",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }
    let branch = match crate::head::head_state(repo)? {
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: sync acts on the branch you are standing on",
                vec!["ff switch <branch>".into()],
            ));
        }
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "target/unresolvable",
                "nothing is committed yet: there is nothing to sync",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Branch { name, .. } => name,
    };
    if crate::held::resolving(repo, &branch)?.is_some() {
        return Err(Error::coded(
            "held/resolving",
            format!(
                "a resolution is open on {branch}: its conflicts are in your working tree, and syncing over them would move the ground they were computed against"
            ),
            vec![
                "ff done".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    if crate::branchmeta::read(repo, &branch)?.session.is_some() {
        return Err(Error::coded(
            "session/open",
            format!("{branch} is an editing session: finish it before syncing"),
            vec!["ff done".into(), "ff done --abandon".into()],
        ));
    }

    let branch_tip = branch_tip(repo, &branch)?;
    let held = crate::held::of(repo, &branch)?.is_some();

    // The remote this branch answers to: the branch's own `remote` first,
    // then the repository default, and only then the refusal.
    let named = repo.branch_remote_name(branch.as_str(), gix::remote::Direction::Fetch);
    let remote = named
        .as_ref()
        .and_then(|name| name.as_symbol())
        .map(|name| name.to_string())
        .or_else(|| {
            repo.remote_default_name(gix::remote::Direction::Fetch)
                .map(|name| name.to_string())
        });
    let remote = match remote {
        Some(name) => Some(name),
        None if repo.remote_names().is_empty() => None,
        None => {
            return Err(Error::coded(
                "sync/ambiguous-remote",
                format!(
                    "{} remotes are configured and none is named origin: fufu will not guess which one {branch} answers to",
                    repo.remote_names().len()
                ),
                vec![
                    "ff git remote -v".into(),
                    "ff git branch --set-upstream-to <remote>/<branch>".into(),
                ],
            ));
        }
    };

    let tracking = match crate::futures::remote_for(repo, &branch)? {
        None => None,
        Some(sync_ref) => {
            let tip = (!sync_ref.tip.is_empty())
                .then(|| gix::ObjectId::from_hex(sync_ref.tip.as_bytes()))
                .transpose()
                .map_err(Error::repo)?;
            let full: gix::refs::FullName = format!("refs/heads/{branch}")
                .as_str()
                .try_into()
                .map_err(Error::repo)?;
            let remote_branch = repo
                .branch_remote_ref_name(full.as_ref(), gix::remote::Direction::Fetch)
                .and_then(|name| name.ok())
                .map(|name| name.as_ref().shorten().to_string())
                .unwrap_or_else(|| branch.clone());
            Some(Tracking {
                full: sync_ref.r#ref,
                name: sync_ref.name,
                remote_branch,
                tip,
            })
        }
    };

    Ok(Preflight {
        branch,
        branch_tip,
        remote,
        tracking,
        held,
    })
}

/// The branch's tip by short name. Both callers have already heard from
/// HEAD that the branch exists, so a miss is fufu's bug, not the user's.
fn branch_tip(repo: &gix::Repository, branch: &str) -> Result<gix::ObjectId> {
    crate::refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        // Uncoded on purpose: a curated id is a promise that a person can
        // reach this and be told what to do about it, and nobody can.
        Error::msg(format!(
            "HEAD is on {branch}, but the branch ref is missing: internal inconsistency"
        ))
    })
}

/// The facts sync cannot learn for itself, handed in by whoever ran the
/// network. Parameters rather than a fetch here is the whole reason this file
/// is testable without a git binary.
pub struct SyncOptions {
    /// `--push` / `--no-push`. `None` reads `fufu.pushOnSync`, default true.
    pub push: Option<bool>,
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
            if diverged && !arrived {
                let ahead = crate::upstream::count_exclusive(repo, pre.branch_tip, &bases)?;
                let behind = crate::upstream::count_exclusive(repo, after, &bases)?;
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

    // The exit. The tip is read again because both axes may have moved it,
    // and a hold either axis just recorded blocks it too.
    let tip_now = branch_tip(repo, &pre.branch)?;
    let publish = if crate::held::of(repo, &pre.branch)?.is_some() {
        // A held rewrite blocks the push whatever the knob says: what the
        // knob never buys is passage.
        Publish::Blocked
    } else if pre.remote.is_none() {
        // Nowhere to send it, so nothing was sent and nothing is waiting.
        // This is a repository that has never had a remote, where the base
        // axis is the whole of sync.
        Publish::Off { pending: false }
    } else {
        // What a push would do, decided before the knob is consulted — so
        // that turning the knob off can still say what it declined to send.
        let would = match pre.remote.as_ref() {
            // Unreachable: the arm above returned for a repository with no
            // remote, and every path below has one in hand.
            None => Publish::UpToDate,
            Some(remote) => match pre.tracking.as_ref() {
                // No upstream at all: the push is what creates one and
                // starts tracking it.
                None => Publish::Create {
                    remote: remote.clone(),
                    remote_branch: pre.branch.clone(),
                    tip: tip_now.to_string(),
                },
                // Configured and absent. Re-creating a branch somebody
                // deleted is a decision rather than a default, and `--push`
                // is how you say it out loud.
                Some(_) if opts.tracking_after.is_none() && opts.push != Some(true) => {
                    Publish::Gone
                }
                Some(_) if opts.tracking_after == Some(tip_now) => Publish::UpToDate,
                Some(tracking) => Publish::Push {
                    remote: remote.clone(),
                    remote_branch: tracking.remote_branch.clone(),
                    // The lease's expected value is exactly the tip the
                    // fetch left behind — what "what I last saw" means — the
                    // same fact the divergence rule read one step earlier.
                    // The empty string, for an explicit `--push` re-creating
                    // a gone branch, is git's own spelling for *must not
                    // exist*.
                    lease: opts
                        .tracking_after
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    tip: tip_now.to_string(),
                },
            },
        };
        let push_wanted = opts.push.unwrap_or_else(|| {
            repo.config_snapshot()
                .boolean("fufu.pushOnSync")
                .unwrap_or(true)
        });
        if push_wanted {
            would
        } else {
            Publish::Off {
                pending: matches!(would, Publish::Create { .. } | Publish::Push { .. }),
            }
        }
    };

    Ok((
        SyncReport {
            branch: pre.branch.clone(),
            fetched: opts.fetched && pre.remote.is_some(),
            remote,
            base,
            publish,
        },
        ctx,
    ))
}
