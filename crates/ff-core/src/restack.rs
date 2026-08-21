//! `ff restack` replays a branch's commits onto a different base — the one
//! recorded for it, or the one `--onto` names — and carries the open change
//! onto the new tip. It is the primitive `ff sync` and `ff done` will aim at,
//! and it is offline: one branch moves, never a cascade of branches.
//!
//! The one rule that organizes this file: the worktree moves if and only if
//! the branch HEAD stands on is carried by the rewrite — always so for the
//! branch you are on, never so for one you are not, and the difference
//! between writing files and touching none at all.

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::futures::{self, At, UnknownReason, Verdict};
use crate::held::{self, Held, Intent};
use crate::model::{HeadState, HeldReport, Parked, RestackOutcome, RestackReport};
use crate::ops::record::{ParentTransition, RefTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, StashEffect, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::stash::{self, ArrivePlan};
use crate::switch;

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

/// A restack that conflicts is an outcome, not an error: record the hold as
/// a slim operation and report it. Nothing moves — no ref, no file, no
/// futures cache — so the whole path is the operation's append and the
/// branch's metadata, the way `ff describe` records a pending description.
fn hold(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    branch: &str,
    at: &At,
    base: &Onto,
    paths: &[String],
    of: usize,
) -> Result<RestackOutcome> {
    let now = rec.now;
    // One hold per branch, and the check sits here — where the hold would be
    // recorded — rather than at the top of the verb, on purpose: a rewrite
    // that would *succeed* while a hold stands is allowed through, because
    // it is not competing for anything and the hold re-derives itself the
    // next time somebody asks.
    held::refuse_if_held(repo, branch, "restacked")?;

    let held = Held {
        intent: Intent::Restack {
            branch: branch.to_string(),
            onto: base.full.clone(),
        },
        at: at.clone(),
        paths: paths.to_vec(),
        time: now,
    };
    held::record(
        repo,
        rec,
        branch,
        &held,
        format!("hold restack of {branch} onto {}", base.name),
    )?;

    Ok(RestackOutcome::Held(HeldReport {
        verb: "restack".into(),
        branch: branch.to_string(),
        at: at.clone(),
        paths: paths.to_vec(),
        of,
    }))
}

/// A base `--onto` may name: the ref to replay onto, and whether that ref is
/// a local branch. A bare name resolves to a local branch, and only a local
/// branch may be re-aimed at — recording a parent is what `--onto` does for
/// a branch. A `refs/`-prefixed spelling is taken as written, which is how
/// `ff sync` aims its remote axis at a tracking ref; a tracking ref is never
/// recorded as a parent, because a branch whose base was its own remote
/// would answer to itself.
pub(crate) struct Onto {
    /// The full ref: `refs/heads/main`, `refs/remotes/origin/feature`.
    pub(crate) full: String,
    /// What a person calls it: `main`, `origin/feature`.
    pub(crate) name: String,
    /// A local branch, and so something a `--onto` may record as a parent.
    pub(crate) local: bool,
    pub(crate) tip: gix::ObjectId,
}

/// Resolve a base by either spelling. A `refs/`-prefixed spelling is taken
/// as written — no prefix matching — because the caller knows exactly what
/// it wants to aim at; anything else is a bare branch name resolved the way
/// every other verb resolves one, so a hold recorded with the old spelling
/// still replans.
pub(crate) fn resolve_onto(repo: &gix::Repository, raw: &str) -> Result<Onto> {
    if let Some(tail) = raw.strip_prefix("refs/") {
        let tip = refs::ref_target(repo, raw)?.ok_or_else(|| {
            Error::coded("branch/not-found", format!("no ref named {raw}"), vec![])
        })?;
        let name = if let Some(heads) = tail.strip_prefix("heads/") {
            heads.to_string()
        } else if let Some(remotes) = tail.strip_prefix("remotes/") {
            remotes.to_string()
        } else {
            raw.to_string()
        };
        Ok(Onto {
            full: raw.to_string(),
            name,
            local: raw.starts_with("refs/heads/"),
            tip,
        })
    } else {
        let name = switch::resolve_branch(repo, raw)?;
        let full = format!("refs/heads/{name}");
        let tip = refs::ref_target(repo, &full)?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {name}"),
                vec![],
            )
        })?;
        Ok(Onto {
            full,
            name,
            local: true,
            tip,
        })
    }
}

/// The base `futures` named for this branch, as an `Onto`.
///
/// The ref comes from the `SyncRef` rather than from `refs/heads/{name}`,
/// because a trunk can be remote-tracking only — `origin/HEAD` with no local
/// branch of that name — and replaying onto a ref the futures probe never
/// measured against would answer a different question than the one `ff
/// status` asked. The *name* comes from the `SyncRef` too, so a
/// remote-qualified trunk still displays as `main`: ref syntax on the screen
/// is exactly what fufu exists to delete.
fn onto_from(repo: &gix::Repository, sync_ref: &futures::SyncRef) -> Result<Onto> {
    let tip = refs::ref_target(repo, &sync_ref.r#ref)?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {}", sync_ref.name),
            vec![],
        )
    })?;
    Ok(Onto {
        full: sync_ref.r#ref.clone(),
        name: sync_ref.name.clone(),
        local: sync_ref.r#ref.starts_with("refs/heads/"),
        tip,
    })
}

/// The triple a restack replays: the oldest commit of the branch that is not
/// already on the base, the branch's tip, and the base to land on.
///
/// `onto` is a ref name, resolved fresh: the base having moved since the hold
/// was recorded is the ordinary case, and reading it again is the whole point
/// of re-planning rather than replaying what was stored.
pub(crate) fn replan_restack(
    repo: &gix::Repository,
    branch: &str,
    onto: &str,
) -> Result<held::Replan> {
    let branch_tip = refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {branch}"),
            vec![],
        )
    })?;
    let base = resolve_onto(repo, onto)?;
    let base_tip = base.tip;
    let base_name = base.name;

    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(branch_tip, &[base_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Err(Error::coded(
            "restack/unrelated",
            format!(
                "{branch} and {base_name} have no common ancestor: there is nothing to replay onto"
            ),
            vec!["ff log".into()],
        ));
    }

    // Oldest-first from the walk, reversed below — the same range the verb
    // measures, taken from the branch's own commits down to the base.
    let walk = repo
        .rev_walk(Some(branch_tip))
        .with_boundary(bases.iter().copied())
        .all()
        .map_err(Error::repo)?;
    let mut range = Vec::new();
    for info in walk {
        range.push(info.map_err(Error::repo)?.id);
    }
    range.reverse(); // oldest-first; the target is the first element
    if range.is_empty() {
        return Err(Error::msg(format!(
            "{branch} already sits on {base_name}: there is nothing to restack"
        )));
    }

    Ok(held::Replan {
        target: range[0],
        tip: branch_tip,
        change: rewrite::Change::Onto(base_tip),
    })
}

/// Replay the branch's commits onto its base, carrying the open change onto
/// the new tip when the head branch is carried along.
pub fn restack(
    repo: &gix::Repository,
    branch: Option<String>,
    onto: Option<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(RestackOutcome, verb::VerbContext)> {
    restack_with(
        repo,
        branch,
        onto,
        prov,
        (now, argv),
        &rewrite::Decided::none(),
    )
}

/// `restack`, with some rewritten commits' trees decided in advance: those
/// skip the three-way merge and take what they are given, and the pre-flight
/// probe — a question about merges that are no longer going to happen — is
/// asked only when nothing is decided.
pub fn restack_with(
    repo: &gix::Repository,
    branch: Option<String>,
    onto: Option<String>,
    prov: &Provenance,
    invocation: (Option<i64>, Vec<String>),
    decided: &rewrite::Decided,
) -> Result<(RestackOutcome, verb::VerbContext)> {
    let (now, argv) = invocation;
    // 1. Guards.
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to restack",
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

    // 3. HEAD, and the branch to move. HEAD's own state is only fatal when
    // the verb needs it: a named branch does not.
    let head = crate::head::head_state(repo)?;
    let (head_branch, head_tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (Some(name.clone()), Some(tip))
        }
        HeadState::Unborn { .. } | HeadState::Detached { .. } => (None, None),
    };

    let branch = match branch {
        Some(raw) => {
            let name = switch::resolve_branch(repo, &raw)?;
            // Restacking a branch another worktree has checked out would
            // rewrite that worktree's tree underfoot.
            crate::branch::guard_other_worktrees(repo, &name)?;
            name
        }
        None => match &head {
            HeadState::Branch { name, .. } => name.clone(),
            HeadState::Unborn { .. } => {
                return Err(Error::coded(
                    "target/unresolvable",
                    "nothing is committed yet: there is nothing to restack",
                    vec!["ff commit -m <msg>".into()],
                ));
            }
            HeadState::Detached { .. } => {
                return Err(Error::coded(
                    "repo/detached",
                    "detached HEAD: name the branch to restack",
                    vec!["ff restack <branch>".into()],
                ));
            }
        },
    };

    let branch_tip = refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {branch}"),
            vec![],
        )
    })?;

    // A session branch sits at the commit it edits, below the branch it
    // will land on: restacking it — replayed or fast-forwarded — would move
    // it off that commit, and `ff done` no longer has the content to fold
    // back. The check sits on the resolved name, so it covers both the bare
    // verb from inside the session and a named restack from elsewhere.
    if branchmeta::read(repo, &branch)?.session.is_some() {
        return Err(Error::coded(
            "session/open",
            format!(
                "{branch} is an editing session: restacking it would move it off the commit \
                 being edited"
            ),
            vec!["ff done".into(), "ff done --abandon".into()],
        ));
    }

    // 4. The base.
    let reaim_requested = onto.is_some();
    let base = match onto {
        Some(raw) => {
            let base = resolve_onto(repo, &raw)?;
            if base.full == format!("refs/heads/{branch}") {
                return Err(Error::coded(
                    "usage/restack-onto-self",
                    format!("{branch} cannot be restacked onto itself"),
                    vec![format!("ff restack {branch} --onto <base>")],
                ));
            }
            base
        }
        None => {
            let sync_ref = futures::base_for(repo, &branch)?.ok_or_else(|| {
                Error::coded(
                    "restack/no-base",
                    format!("{branch} has no base to replay onto"),
                    vec![
                        format!("ff restack {branch} --onto <base>"),
                        "ff status".into(),
                    ],
                )
            })?;
            onto_from(repo, &sync_ref)?
        }
    };
    // Only `--onto` asks for a base to be written down: the bare verb replays
    // onto the base already recorded, which `ff start` wrote and which may
    // name someone else's branch. What survives of the old local-only rule is
    // narrower and still load-bearing — the base axis and the remote axis
    // must never aim at the same ref — and `--onto` keeps it by resolving
    // branches by name through its own path, which refuses a tracking ref.
    let reaimed = reaim_requested && base.local;
    let base_tip = base.tip;
    let base_name = base.name.clone();

    let recorded_parent = branchmeta::read(repo, &branch)?.parent;
    let previous_parent = recorded_parent.clone();
    // `--onto` naming the parent that is already recorded changes nothing.
    let parent_changes = reaimed && recorded_parent.as_deref() != Some(base_name.as_str());

    // 5. The range, and whether the worktree moves.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(branch_tip, &[base_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Err(Error::coded(
            "restack/unrelated",
            format!(
                "{branch} and {base_name} have no common ancestor: there is nothing to \
                     replay onto"
            ),
            vec!["ff log".into()],
        ));
    }

    // Up-to-date before fast-forward: when the tips are equal both are true,
    // and the honest answer is "already there", not "fast-forwarded by
    // nothing". futures.rs makes the same choice for the same reason.
    let up_to_date = bases.contains(&base_tip);
    let fast_forward = !up_to_date && bases.contains(&branch_tip);

    if up_to_date && !parent_changes {
        return Ok((
            RestackOutcome::NothingToRestack {
                branch,
                base: base_name,
            },
            ctx,
        ));
    }

    let mut range: Vec<gix::ObjectId> = Vec::new();
    if !up_to_date && !fast_forward {
        // Newest-first from the walk; reversed below. A restack is something
        // a person asked for, so the range is walked in full — no depth cap.
        let walk = repo
            .rev_walk(Some(branch_tip))
            .with_boundary(bases.iter().copied())
            .all()
            .map_err(Error::repo)?;
        for info in walk {
            let info = info.map_err(Error::repo)?;
            if info.parent_ids().count() > 1 {
                return Err(Error::coded(
                    "rewrite/merge-in-range",
                    format!(
                        "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                         rewritten",
                        crate::sha::short_oid(info.id),
                        subject(repo, info.id)?
                    ),
                    vec!["ff log".into()],
                ));
            }
            range.push(info.id);
        }
        range.reverse(); // oldest-first; the target is the first element
    }

    let head_carried = if up_to_date {
        false
    } else if fast_forward {
        head_branch.as_deref() == Some(branch.as_str())
    } else {
        head_branch
            .as_deref()
            .is_some_and(|h| h == branch || head_tip.is_some_and(|t| range.contains(&t)))
    };

    // 6. The open change — only when the head branch is carried.
    let (mut open, mut head_tip_tree) = (None, None);
    if head_carried && let Some(ht) = head_tip {
        let ht_tree = tree_of(repo, ht)?;
        open = Some(open_tree(repo, ht_tree)?.0);
        head_tip_tree = Some(ht_tree);
    }

    // 7. The conflict verdict — probes only, nothing written. A verb the user
    // asked for pays the full replay cost, unlike thrifty `ff status`. Skipped
    // for a decided landing: its trees are already known, so the replay has
    // nothing left to conflict on, and probing would answer about a merge
    // that is no longer going to happen.
    let mut replayed = 0usize;
    if decided.is_empty() && !up_to_date && !fast_forward {
        let probe_open = if head_carried && head_branch.as_deref() == Some(branch.as_str()) {
            open
        } else {
            None
        };
        match futures::probe_to_depth(repo, base_tip, branch_tip, probe_open, usize::MAX)? {
            Verdict::Clean { .. } => {
                // Standing mid-stack: the open change belongs to the head
                // branch's tip, not the target's, so it needs its own probe.
                // Replaying bases..head_tip is a prefix of the range just
                // proven clean, so only the open-change step can still fail.
                if let (Some(open_t), Some(hb), Some(ht)) = (open, head_branch.as_deref(), head_tip)
                    && hb != branch
                    && let Verdict::Conflict {
                        at: at @ At::OpenChange,
                        paths,
                    } = futures::probe_to_depth(repo, base_tip, ht, Some(open_t), usize::MAX)?
                {
                    return Ok((
                        hold(
                            repo,
                            held::Recording {
                                ctx: &ctx,
                                prov,
                                argv,
                                now,
                            },
                            &branch,
                            &at,
                            &base,
                            &paths,
                            range.len(),
                        )?,
                        ctx,
                    ));
                }
            }
            Verdict::Conflict { at, paths } => {
                return Ok((
                    hold(
                        repo,
                        held::Recording {
                            ctx: &ctx,
                            prov,
                            argv,
                            now,
                        },
                        &branch,
                        &at,
                        &base,
                        &paths,
                        range.len(),
                    )?,
                    ctx,
                ));
            }
            // Unreachable in practice — §5 already refused a merge in the
            // range — but handled rather than panicked, named the way §5
            // names it.
            Verdict::Unknown {
                reason: UnknownReason::MergeCommits,
            } => {
                let merge = range
                    .iter()
                    .copied()
                    .find(|&id| {
                        repo.find_object(id).is_ok_and(|obj| {
                            gix::objs::CommitRef::from_bytes(&obj.data)
                                .is_ok_and(|c| c.parents.len() > 1)
                        })
                    })
                    .unwrap_or(branch_tip);
                return Err(Error::coded(
                    "rewrite/merge-in-range",
                    format!(
                        "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                         rewritten",
                        crate::sha::short_oid(merge),
                        subject(repo, merge).unwrap_or_default()
                    ),
                    vec!["ff log".into()],
                ));
            }
            verdict => {
                return Err(Error::msg(format!(
                    "the probe and the range walk disagree ({verdict:?}): internal inconsistency"
                )));
            }
        }
    }

    // 8. Plan, and the new worktree.
    let mut carried: Vec<RefTransition> = Vec::new();
    let mut rewrites: Vec<rewrite::Rewrite> = Vec::new();
    let mut dropped: Vec<rewrite::Dropped> = Vec::new();
    let mut new_tip = branch_tip;
    let mut published = 0usize;
    let mut new_head_tip: Option<gix::ObjectId> = None;
    let mut new_worktree: Option<gix::ObjectId> = None;

    if !up_to_date && !fast_forward {
        // The triple the restack replays — the same one `held::replan`
        // re-derives, so the verb and the replan cannot disagree.
        let replan = replan_restack(repo, &branch, &base.full)?;
        let plan = rewrite::plan_with(
            repo,
            replan.target,
            replan.tip,
            &replan.change,
            now,
            &decided.trees,
        )?;
        published = rewrite::published_count(repo, &branch, &plan)?;
        // The probe and the plan agree by construction, but only one of them
        // is the thing that ran: the counts come from the plan.
        carried = plan.carried;
        rewrites = plan.rewrites;
        replayed = rewrites.len();
        dropped = plan.dropped;
        new_tip = plan.new_tip;

        // The head branch's new tip comes out of the carried table either
        // way, so there is one code path for head and non-head.
        if head_carried && let Some(hb) = head_branch.as_deref() {
            let hb_ref = format!("refs/heads/{hb}");
            let entry = carried
                .iter()
                .find(|t| t.name == hb_ref)
                .ok_or_else(|| Error::msg(format!("{hb_ref} was not carried: internal error")))?;
            let new_hex = entry
                .new
                .as_deref()
                .ok_or_else(|| Error::msg(format!("{hb_ref} has no new end: internal error")))?;
            new_head_tip = Some(gix::ObjectId::from_hex(new_hex.as_bytes()).map_err(Error::repo)?);
        }

        // The worktree the verb will leave behind. §7 already proved this
        // merge clean, so this pass only makes it real.
        if let (Some(open_t), Some(ht_tree), Some(new_ht)) = (open, head_tip_tree, new_head_tip) {
            if open_t == ht_tree {
                new_worktree = Some(tree_of(repo, new_ht)?);
            } else {
                let mut outcome = merge_into(repo, ht_tree, tree_of(repo, new_ht)?, open_t)?;
                new_worktree = Some(outcome.tree.write().map_err(Error::repo)?.detach());
            }
        }
    } else if fast_forward {
        // The base already contains the branch: no commit is rewritten, the
        // ref simply moves. No plan, no rewrite map.
        carried = vec![RefTransition {
            name: format!("refs/heads/{branch}"),
            old: Some(branch_tip.to_string()),
            new: Some(base_tip.to_string()),
        }];
        new_tip = base_tip;
        if head_carried && let (Some(open_t), Some(ht_tree)) = (open, head_tip_tree) {
            new_head_tip = Some(base_tip);
            // The FastForward verdict fires before the probe ever reaches the
            // open-change step, so it is checked here — purely.
            let base_tree = ht_tree;
            let ours_tree = tree_of(repo, base_tip)?;
            let paths = futures::conflict_paths(repo, base_tree, ours_tree, open_t)?;
            if !paths.is_empty() {
                // A fast-forward restacks no commit, so the stack the report
                // sizes is empty: range was never built and stands at zero.
                return Ok((
                    hold(
                        repo,
                        held::Recording {
                            ctx: &ctx,
                            prov,
                            argv,
                            now,
                        },
                        &branch,
                        &At::OpenChange,
                        &base,
                        &paths,
                        range.len(),
                    )?,
                    ctx,
                ));
            }
            if open_t == ht_tree {
                new_worktree = Some(ours_tree);
            } else {
                let mut outcome = merge_into(repo, base_tree, ours_tree, open_t)?;
                new_worktree = Some(outcome.tree.write().map_err(Error::repo)?.detach());
            }
        }
    }

    // 10b. A resolution's park comes home. `ff resolve` parks the open change
    // a restack CARRIES, to make room for the markers; this is the other half
    // of that rule, and it runs inside the landing's own operation so one
    // `ff undo` takes the park and the restack back together. An ordinary
    // restack parked nothing — §13 only discloses what `ff switch` left
    // behind — so it plans no arrival at all.
    let arrive_target = new_worktree
        .map(Ok)
        .unwrap_or_else(|| tree_of(repo, new_tip))?;
    let arrive_plan =
        if decided.clearing.is_some() && head_branch.as_deref() == Some(branch.as_str()) {
            stash::plan_arrival(repo, &branch, new_tip, arrive_target)?
        } else {
            ArrivePlan::None
        };

    // 11. Write-ahead: the planned table is the post-restack world.
    let mut planned = observe_refs(repo)?;
    for t in &carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let mut refs_transitions: Vec<RefTransition> = carried.clone();
    let mut stash_effects: Vec<StashEffect> = Vec::new();
    if !matches!(arrive_plan, ArrivePlan::None) {
        let mut stash_lines: Vec<gix::ObjectId> = refs::read_ref_log(repo, stash::STASH_REF)?
            .iter()
            .map(|l| l.new)
            .collect();
        match &arrive_plan {
            ArrivePlan::Restore { stash: sha, .. } => {
                if let Some(pos) = stash_lines.iter().rposition(|s| s == sha) {
                    stash_lines.remove(pos);
                }
                planned.refs.remove(&stash::parked_ref(&branch));
                refs_transitions.push(RefTransition {
                    name: stash::parked_ref(&branch),
                    old: Some(sha.to_string()),
                    new: None,
                });
                stash_effects.push(StashEffect::Drop {
                    branch: branch.clone(),
                    stash: sha.to_string(),
                });
                match stash_lines.last() {
                    Some(t) => {
                        planned
                            .refs
                            .insert(stash::STASH_REF.to_string(), t.to_string());
                    }
                    None => {
                        planned.refs.remove(stash::STASH_REF);
                    }
                }
            }
            ArrivePlan::Invalidate { stash: sha } => {
                planned.refs.remove(&stash::parked_ref(&branch));
                refs_transitions.push(RefTransition {
                    name: stash::parked_ref(&branch),
                    old: Some(sha.to_string()),
                    new: None,
                });
            }
            ArrivePlan::None | ArrivePlan::Conflict { .. } => {}
        }
    }

    let mut record = OpRecord::new("restack", format!("restack {branch} onto {base_name}"), now);
    record.argv = argv;
    record.refs = refs_transitions;
    record.stash = stash_effects;
    record.rewrites = rewrites.clone();
    record.dropped = dropped.clone();
    if parent_changes {
        record.parent = Some(ParentTransition {
            branch: branch.clone(),
            old: recorded_parent.clone(),
            new: Some(base_name.clone()),
        });
    }
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }

    let mut pins: Vec<gix::ObjectId> = rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(branch_tip);
    if let Some(head_tip) = head_tip
        && head_tip != branch_tip
    {
        pins.push(head_tip);
    }
    match &arrive_plan {
        ArrivePlan::Restore { stash, .. }
        | ArrivePlan::Conflict { stash, .. }
        | ArrivePlan::Invalidate { stash } => pins.push(*stash),
        ArrivePlan::None => {}
    }

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Absorb passes ctx.pre_tree because it writes no files; restack
            // does when the head branch is carried. The recorded end tree
            // must be what the tree will actually hold, or an undo of the
            // next operation throws the carried change away (switch.rs:203-214).
            tree: match &arrive_plan {
                ArrivePlan::Restore { target_wip, .. } => *target_wip,
                _ => new_worktree.unwrap_or(ctx.pre_tree),
            },
            // The new tip's tree, so the next foreign `git status` sees the
            // open change against the commit it now sits on.
            index_tree: match &arrive_plan {
                ArrivePlan::Restore { target_index, .. } => *target_index,
                _ => new_head_tip
                    .map(|id| tree_of(repo, id))
                    .transpose()?
                    .unwrap_or(ctx.pre_tree),
            },
            // The chain the op leaves you on: an off-branch restack leaves
            // you exactly where you stood, so HEAD's branch — and only when
            // HEAD is detached or unborn is the restacked one the honest
            // answer.
            branch: head_branch.clone().unwrap_or_else(|| branch.clone()),
            base: Some(branch_tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 12. Mutate.
    // 12.1 Refs: one atomic transaction over every carried head.
    let reflog_msg = format!("restack: onto {base_name}");
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
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while restacking; nothing was rewritten (re-run to restack on the \
                 new tips)",
                vec![],
            ));
        }
    }

    // 12.2 The recorded parent.
    if parent_changes {
        let mut meta = branchmeta::read(repo, &branch)?;
        meta.parent = Some(base_name.clone());
        branchmeta::write(repo, &branch, &meta)?;
    }

    // 12.3 Worktree: index first, then the files — the order switch.rs uses.
    let mut files = 0usize;
    if let (Some(open_t), Some(new_wt), Some(new_ht)) = (open, new_worktree, new_head_tip) {
        crate::index::write_index_for_tree(repo, tree_of(repo, new_ht)?)?;
        let everything = |_: &str| true;
        let transition = crate::worktree::apply_tree_transition(repo, open_t, new_wt, &everything)?;
        files = transition.written.len() + transition.deleted.len();
    }

    // 12.3b The parked change comes back, exactly the way `ff switch` brings
    // one back — same plan, same executor, same journal entries.
    let arrival = stash::execute_arrival(repo, &branch, &arrive_plan, arrive_target, now)?;

    // 12.4 Futures caches: the restacked branch and every branch it carried.
    // Best-effort — it costs recomputation and nothing else.
    let _ = futures::cache::remove(repo, &branch);
    for t in &carried {
        if let Some(name) = t.name.strip_prefix("refs/heads/")
            && name != branch
        {
            let _ = futures::cache::remove(repo, name);
        }
    }

    // 12.5 A resolution landing: clear the hold and the session it resolved,
    // so one `ff undo` of this op takes the whole resolution back.
    if let Some(clearing) = &decided.clearing {
        held::set(repo, &clearing.branch, None)?;
        held::set_resolving(repo, &clearing.branch, None)?;
    }

    // 13. The parked disclosure: say so, never move it. Skipped for the
    // branch underfoot — a branch you are standing on has no parked entry to
    // speak of, its change being open rather than parked.
    let parked = if head_carried && head_branch.as_deref() == Some(branch.as_str()) {
        // A branch you are standing on has no parked entry to speak of, its
        // change being open rather than parked — unless a resolution parked
        // one and the arrival above could not put it back, which is the one
        // thing here that has to be said out loud.
        match &arrival {
            crate::stash::Arrival::Conflicted { stash, .. } => Some(Parked {
                stash: stash.clone(),
                applies: false,
            }),
            _ => None,
        }
    } else {
        match crate::stash::parked_entry(repo, &branch)? {
            Some(id) => {
                let sc = crate::stash::read_stash_commit(repo, id)?;
                let new_tip_tree = tree_of(repo, new_tip)?;
                let applies =
                    futures::conflict_paths(repo, sc.base_tree, new_tip_tree, sc.wip_tree)?
                        .is_empty();
                Some(Parked {
                    stash: id.to_string(),
                    applies,
                })
            }
            None => None,
        }
    };

    // 14. The report.
    let behind = crate::upstream::count_exclusive(repo, base_tip, &bases)?;
    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect(); // carried is already sorted by ref name

    // What is still open is what the worktree will hold once this lands,
    // measured against the commit it will then sit on — not the tree the
    // change stood against before the replay. Comparing the *old* open tree
    // to the *new* tip answers "did the replay move anything", which is true
    // of every replay that did its job, and would report a clean tree as
    // dirty. Both trees here are exact rather than `ctx.pre_tree`: the
    // capture floor may have size-capped a blob out of pre_tree while the
    // exact tree kept it.
    let still_open = match (
        new_worktree,
        new_head_tip.map(|id| tree_of(repo, id)).transpose()?,
    ) {
        (Some(worktree), Some(tip_tree)) => worktree != tip_tree,
        _ => false,
    };

    let published_on = rewrite::tracking_name(repo, &branch)?;

    Ok((
        RestackOutcome::Restacked(Box::new(RestackReport {
            branch,
            base: base_name,
            onto: base_tip.to_string(),
            reaimed,
            previous_parent,
            replayed,
            behind,
            fast_forward,
            new_tip: new_tip.to_string(),
            moved,
            published,
            published_on,
            parked,
            files,
            still_open,
            dropped,
        })),
        ctx,
    ))
}
