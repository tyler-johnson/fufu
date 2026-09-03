//! `ff restack` replays a branch's commits onto a different base — the one
//! recorded for it, or the one `--onto` names — and carries the open change
//! onto the new tip. It is the primitive `ff sync` and `ff done` will aim at,
//! and it is offline. The branches stacked above the one it moves follow it:
//! `cascade` plans their replay once the new tip is known and before the
//! operation is written, so the whole cascade rides this one operation.
//!
//! The one rule that organizes this file: the worktree moves if and only if
//! the branch HEAD stands on is carried by the rewrite — always so for the
//! branch you are on, never so for one you are not, and the difference
//! between writing files and touching none at all.

use crate::branchmeta;
use crate::cascade::{self, CascadePlan};
use crate::error::{Error, Result};
use crate::futures::{self, At, Verdict};
use crate::held::{self, Held, Intent};
use crate::model::{HeadState, HeldReport, Parked, RestackOutcome, RestackReport};
use crate::ops::record::{ParentTransition, RefTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, StashEffect, verb};
use crate::overlay::Overlay;
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::stash::{self, ArrivePlan};
use crate::switch;
use serde::Serialize;

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
    plan: &HoldPlan,
) -> Result<RestackOutcome> {
    held::record(repo, rec, &plan.branch, &plan.held, plan.summary())?;
    Ok(RestackOutcome::Held(plan.report.clone()))
}

/// A base `--onto` may name: the ref to replay onto, and what to call it.
/// A bare name resolves to a local branch first, then to the tracking ref
/// under the same name — `origin/main` is a base, and `--onto`
/// records it as a parent whatever namespace it lives in. What refuses the
/// aim is not the namespace but the branch's own shared copy: a branch whose
/// base was its own remote would answer to itself. A `refs/`-prefixed
/// spelling is taken as written, which is how `ff sync` aims its remote axis
/// at a tracking ref.
pub(crate) struct Onto {
    /// The full ref: `refs/heads/main`, `refs/remotes/origin/feature`.
    pub(crate) full: String,
    /// What a person calls it: `main`, `origin/feature`.
    pub(crate) name: String,
    pub(crate) tip: gix::ObjectId,
}

/// Who aimed this restack. It settles exactly one question — whether aiming
/// a branch at its own shared copy is refused. A person who typed `--onto`
/// just now meant a different base to sit on, so the refusal is owed to
/// them. Machinery — `ff sync`'s remote axis, or a hold being resumed —
/// aimed at that ref on purpose: reconciling with the remote is its whole
/// job, and its aim was settled when the sync or the hold was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Aim {
    /// A person typed `--onto`.
    Asked,
    /// Machinery aimed this: `ff sync`'s remote axis, or a hold being resumed.
    Settled,
}

/// Resolve a base by either spelling. A `refs/`-prefixed spelling is taken
/// as written — no prefix matching — because the caller knows exactly what
/// it wants to aim at. Anything else is a local branch name first, resolved
/// the way every other verb resolves one, and only a bare not-found falls
/// through to the tracking ref under the same name — so `origin/main` and
/// `refs/remotes/origin/main` mean the same base, and a hold recorded with
/// either spelling still replans.
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
            tip,
        })
    } else {
        // A local name wins over a same-named tracking ref, and a local
        // ambiguity is a real ambiguity: it stays an error rather than
        // falling through to a remote guess. Only a bare not-found falls
        // through, to the tracking ref under the same name — the spelling a
        // person types, and the one that lands on the screen and in the
        // recorded parent.
        let name = match switch::resolve_branch(repo, raw) {
            Ok(name) => name,
            Err(err) if err.id() != "branch/not-found" => return Err(err),
            Err(err) => {
                // `<remote>/HEAD` is symbolic and not a branch: hand back
                // the not-found instead of the symbolic-ref error a lookup
                // would raise.
                if refs::is_symbolic(repo, &format!("refs/remotes/{raw}"))? {
                    return Err(err);
                }
                let (full, tip) = refs::branchish(repo, raw)?.ok_or(err)?;
                return Ok(Onto {
                    full,
                    name: raw.to_string(),
                    tip,
                });
            }
        };
        let full = format!("refs/heads/{name}");
        let tip = refs::ref_target(repo, &full)?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {name}"),
                vec![],
            )
        })?;
        Ok(Onto { full, name, tip })
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
        tip,
    })
}

/// Where the range walk stops: the merge bases with the base as it stands,
/// and, when the base is one the branch follows, the point the branch forked
/// from the base's history.
///
/// A rewritten base keeps none of its old commits, so the merge base with
/// its new tip is the point where the base itself forked from trunk, and a
/// walk bounded there hands back the base's own old commits as if they were
/// the branch's. Replayed onto their rewritten selves they drop as empty
/// under a pure move and conflict under a rewrite that changed their
/// content, which is the cascade's reason for bounding a child's range at
/// the base as it stood. A replan has no old tip in hand, so it asks the
/// base's reflog, the way `git rebase --fork-point` does: every place the
/// base has stood is a candidate, and the branch's fork from any of them
/// bounds the walk. A commit the base once held and the branch still sits
/// on is the base's, not the branch's.
///
/// Only the two refs a branch answers to are read this way, its recorded
/// base and its own shared copy. `--onto` aimed elsewhere is a transplant,
/// and a transplant carries everything above the common ancestor.
fn range_boundary(
    repo: &gix::Repository,
    branch: &str,
    branch_tip: gix::ObjectId,
    base: &Onto,
    bases: &[gix::ObjectId],
) -> Result<Vec<gix::ObjectId>> {
    let mut boundary: Vec<gix::ObjectId> = bases.to_vec();
    let followed = futures::base_for(repo, branch)?.is_some_and(|s| s.r#ref == base.full)
        || futures::remote_for(repo, branch)?.is_some_and(|s| s.r#ref == base.full);
    if !followed {
        return Ok(boundary);
    }
    let mut positions: Vec<gix::ObjectId> = vec![base.tip];
    for line in refs::read_ref_log(repo, &base.full)? {
        positions.push(line.new);
        positions.extend(line.previous);
    }
    positions.sort();
    positions.dedup();
    // The branch's own tip as a past position of the base says nothing about
    // where the branch forked, and gix answers a `first` inside `others`
    // with `first` alone.
    positions.retain(|p| *p != branch_tip);
    for id in repo
        .merge_bases_many(branch_tip, &positions)
        .map_err(Error::repo)?
    {
        let id = id.detach();
        if !boundary.contains(&id) {
            boundary.push(id);
        }
    }
    Ok(boundary)
}

/// The commits above `boundary` from `branch_tip` down, oldest first. A
/// merge in the range is refused by the verb and skipped by the cascade;
/// here it is reported so the caller decides.
fn walk_range(
    repo: &gix::Repository,
    branch_tip: gix::ObjectId,
    boundary: &[gix::ObjectId],
) -> Result<(Vec<gix::ObjectId>, Option<gix::ObjectId>)> {
    let walk = repo
        .rev_walk(Some(branch_tip))
        .with_boundary(boundary.iter().copied())
        .all()
        .map_err(Error::repo)?;
    let mut range = Vec::new();
    let mut merge = None;
    for info in walk {
        let info = info.map_err(Error::repo)?;
        if merge.is_none() && info.parent_ids().count() > 1 {
            merge = Some(info.id);
        }
        range.push(info.id);
    }
    range.reverse(); // oldest-first; the target is the first element
    Ok((range, merge))
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
                "{branch} and {} have no common ancestor: there is nothing to replay onto",
                base.name
            ),
            vec!["ff log".into()],
        ));
    }

    // The same range the verb measures: the branch's own commits, down to
    // where it forked from the base.
    let boundary = range_boundary(repo, branch, branch_tip, &base, &bases)?;
    let (range, _merge) = walk_range(repo, branch_tip, &boundary)?;
    let base_name = base.name;
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
        Aim::Asked,
    )
}

/// `restack`, with the things the plain verb decides for you settled by the
/// caller: some rewritten commits' trees — those skip the three-way merge
/// and take what they are given, and the pre-flight probe, a question about
/// merges that are no longer going to happen, is asked only when nothing is
/// decided — and the aim, which settles whether aiming at the branch's own
/// shared copy is refused.
///
/// Two halves: [`plan_restack`] decides everything and writes only commit
/// objects, and the committing half below writes the operation ahead of
/// the refs, the refs in one transaction, and the worktree last. `ff sync`
/// calls the planning half once per axis of every branch against one
/// overlay and commits the lot as its own one operation.
pub fn restack_with(
    repo: &gix::Repository,
    branch: Option<String>,
    onto: Option<String>,
    prov: &Provenance,
    invocation: (Option<i64>, Vec<String>),
    decided: &rewrite::Decided,
    aim: Aim,
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

    // 3 to 10. The plan, against the repository as it stands.
    let plan = plan_restack(repo, branch, onto, now, decided, aim, &Overlay::default())?;
    match plan {
        RestackPlan::Unchanged { branch, base } => {
            Ok((RestackOutcome::NothingToRestack { branch, base }, ctx))
        }
        RestackPlan::Held(plan) => Ok((
            hold(
                repo,
                held::Recording {
                    ctx: &ctx,
                    prov,
                    argv,
                    now,
                },
                &plan,
            )?,
            ctx,
        )),
        RestackPlan::Replay(plan) => {
            let report = commit_restack(repo, &ctx, prov, argv, decided, *plan)?;
            Ok((RestackOutcome::Restacked(Box::new(report)), ctx))
        }
    }
}

/// What a restack decided, before anything is written.
pub(crate) enum RestackPlan {
    /// The branch already sits on its base, and nothing else was asked.
    Unchanged { branch: String, base: String },
    /// The replay conflicts: nothing moves, and the hold is what gets
    /// recorded.
    Held(Box<HoldPlan>),
    /// A replay or a fast-forward, with everything it moves.
    Replay(Box<ReplayPlan>),
}

/// A hold a restack planned.
pub(crate) struct HoldPlan {
    pub branch: String,
    base_name: String,
    pub held: Held,
    pub report: HeldReport,
}

impl HoldPlan {
    /// The summary of the operation that records it.
    pub(crate) fn summary(&self) -> String {
        format!("hold restack of {} onto {}", self.branch, self.base_name)
    }
}

/// A replay a restack planned and has not written: the branch's own ref
/// move, its rewrites and drops, the branches stacked above, and the
/// worktree the head branch carries. Commit objects are written; no ref
/// has moved.
pub(crate) struct ReplayPlan {
    pub branch: String,
    base: Onto,
    branch_tip: gix::ObjectId,
    pub new_tip: gix::ObjectId,
    bases: Vec<gix::ObjectId>,
    /// The restacked branch's own transition.
    pub carried: Vec<RefTransition>,
    pub rewrites: Vec<rewrite::Rewrite>,
    pub dropped: Vec<rewrite::Dropped>,
    diverged: Vec<String>,
    replayed: usize,
    published: usize,
    fast_forward: bool,
    reaimed: bool,
    parent_changes: bool,
    recorded_parent: Option<String>,
    head_branch: Option<String>,
    head_tip: Option<gix::ObjectId>,
    head_carried: bool,
    /// The open change as the plan found it, the tip HEAD's branch will
    /// stand on, and the tree the worktree will hold: all three when the
    /// head branch is carried, by this replay or by its cascade, and none
    /// otherwise.
    pub open: Option<gix::ObjectId>,
    pub new_head_tip: Option<gix::ObjectId>,
    pub new_worktree: Option<gix::ObjectId>,
    pub cascade: CascadePlan,
    arrive_plan: ArrivePlan,
    arrive_target: gix::ObjectId,
}

impl ReplayPlan {
    /// The commits the operation must keep reachable for the branch's own
    /// replay: every rewritten commit and every old tip. The cascade's are
    /// on the cascade plan, folded in beside them.
    pub(crate) fn own_pins(&self) -> Result<Vec<gix::ObjectId>> {
        let mut pins: Vec<gix::ObjectId> = self
            .rewrites
            .iter()
            .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
            .collect::<Result<_>>()?;
        pins.push(self.branch_tip);
        if let Some(head_tip) = self.head_tip
            && head_tip != self.branch_tip
        {
            pins.push(head_tip);
        }
        match &self.arrive_plan {
            ArrivePlan::Restore { stash, .. }
            | ArrivePlan::Conflict { stash, .. }
            | ArrivePlan::Invalidate { stash } => pins.push(*stash),
            ArrivePlan::None => {}
        }
        Ok(pins)
    }

    /// The report, once the plan has landed. `files` is what the worktree
    /// write touched, and `arrival` is what became of a parked change the
    /// landing brought home, when one did.
    pub(crate) fn report(
        &self,
        repo: &gix::Repository,
        files: usize,
        arrival: Option<&stash::Arrival>,
    ) -> Result<RestackReport> {
        let branch = &self.branch;
        // 13. The parked disclosure: say so, never move it. Skipped for the
        // branch underfoot — a branch you are standing on has no parked entry
        // to speak of, its change being open rather than parked.
        let parked = if self.head_carried && self.head_branch.as_deref() == Some(branch.as_str()) {
            // A branch you are standing on has no parked entry to speak of,
            // its change being open rather than parked — unless a resolution
            // parked one and the arrival could not put it back, which is the
            // one thing here that has to be said out loud.
            match arrival {
                Some(crate::stash::Arrival::Conflicted { stash, .. }) => Some(Parked {
                    stash: stash.clone(),
                    applies: false,
                }),
                _ => None,
            }
        } else {
            match crate::stash::parked_entry(repo, branch)? {
                Some(id) => {
                    let sc = crate::stash::read_stash_commit(repo, id)?;
                    let new_tip_tree = tree_of(repo, self.new_tip)?;
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
        let behind = crate::upstream::count_exclusive(repo, self.base.tip, &self.bases)?;
        let branch_ref = format!("refs/heads/{branch}");
        let moved: Vec<String> = self
            .carried
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
        // change stood against before the replay. Comparing the *old* open
        // tree to the *new* tip answers "did the replay move anything", which
        // is true of every replay that did its job, and would report a clean
        // tree as dirty. Both trees here are exact rather than
        // `ctx.pre_tree`: the capture floor may have size-capped a blob out of
        // pre_tree while the exact tree kept it.
        let still_open = match (
            self.new_worktree,
            self.new_head_tip.map(|id| tree_of(repo, id)).transpose()?,
        ) {
            (Some(worktree), Some(tip_tree)) => worktree != tip_tree,
            _ => false,
        };

        let published_on = rewrite::tracking_name(repo, branch)?;

        Ok(RestackReport {
            branch: branch.clone(),
            base: self.base.name.clone(),
            onto: self.base.tip.to_string(),
            reaimed: self.reaimed,
            previous_parent: self.recorded_parent.clone(),
            replayed: self.replayed,
            behind,
            fast_forward: self.fast_forward,
            new_tip: self.new_tip.to_string(),
            moved,
            diverged: self.diverged.clone(),
            published: self.published,
            published_on,
            parked,
            files,
            still_open,
            dropped: self.dropped.clone(),
            cascade: self.cascade.report.clone(),
        })
    }
}

/// The planning half of a restack: everything `restack_with` decides,
/// against the repository as `overlay` says it stands, with commit objects
/// written and no ref moved. A hold is a plan too, and so is a branch that
/// already sits on its base; the one hold per branch rule is checked here,
/// where the hold would be recorded, so a rewrite that would succeed while a
/// hold stands is allowed through.
pub(crate) fn plan_restack(
    repo: &gix::Repository,
    branch: Option<String>,
    onto: Option<String>,
    now: i64,
    decided: &rewrite::Decided,
    aim: Aim,
    overlay: &Overlay,
) -> Result<RestackPlan> {
    // 3. HEAD, and the branch to move. HEAD's own state is only fatal when
    // the verb needs it: a named branch does not. The tip is the overlay's
    // when the run already moved HEAD's branch.
    let head = crate::head::head_state(repo)?;
    let (head_branch, head_tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = match overlay.branch_tip(repo, name)? {
                Some(tip) => tip,
                None => gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
            };
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

    let branch_tip = overlay.branch_tip(repo, &branch)?.ok_or_else(|| {
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
    let (mut base, own_copy) = match onto {
        Some(raw) => {
            let base = resolve_onto(repo, &raw)?;
            if base.full == format!("refs/heads/{branch}") {
                return Err(Error::coded(
                    "usage/restack-onto-self",
                    format!("{branch} cannot be restacked onto itself"),
                    vec![format!("ff restack {branch} --onto <base>")],
                ));
            }
            // Aiming a branch at its own shared copy is reconciling with the
            // remote, not picking a base — refused only when a person typed
            // it: `ff sync`'s remote axis aims exactly there on purpose, and
            // a resumed hold may well be one.
            let own_copy =
                futures::remote_for(repo, &branch)?.is_some_and(|own| own.r#ref == base.full);
            if own_copy && aim == Aim::Asked {
                return Err(Error::coded(
                    "restack/own-remote",
                    format!(
                        "{branch} cannot be restacked onto its own shared copy, {}",
                        base.name
                    ),
                    vec![
                        "ff sync".into(),
                        format!("ff restack {branch} --onto <base>"),
                    ],
                ));
            }
            (base, own_copy)
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
            // `base_for` refuses the branch's own shared copy itself, so the
            // bare verb never aims at one.
            (onto_from(repo, &sync_ref)?, false)
        }
    };
    // The base as the run has planned it, when it moved the base already.
    if let Some(tip) = overlay.tip(&base.full) {
        base.tip = tip;
    }
    // `--onto` records a parent whatever namespace the base lives in: `ff
    // start origin/x` already writes one, and `base_for` already resolves
    // one. What survives of the old local-only rule is the invariant
    // `base_for` enforces — the base axis and the remote axis must never aim
    // at the same ref — and being a local branch never was that test.
    let reaimed = reaim_requested && !own_copy;
    let base_tip = base.tip;
    let base_name = base.name.clone();

    let recorded_parent = branchmeta::read(repo, &branch)?.parent;
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
        return Ok(RestackPlan::Unchanged {
            branch,
            base: base_name,
        });
    }

    let mut range: Vec<gix::ObjectId> = Vec::new();
    if !up_to_date && !fast_forward {
        // The branch's own commits, down to where it forked from the base:
        // the range `replan_restack` measures, so the verb and the replan
        // cannot disagree. A restack is something a person asked for, so
        // the range is walked in full — no depth cap.
        let boundary = range_boundary(repo, &branch, branch_tip, &base, &bases)?;
        let (walked, merge) = walk_range(repo, branch_tip, &boundary)?;
        if let Some(merge) = merge {
            return Err(Error::coded(
                "rewrite/merge-in-range",
                format!(
                    "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                     rewritten",
                    crate::sha::short_oid(merge),
                    subject(repo, merge)?
                ),
                vec!["ff log".into()],
            ));
        }
        range = walked;
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

    // 6. The open change — only when the head branch is carried. The
    // overlay answers with the worktree it has planned when the run already
    // moved HEAD's branch, and with the files otherwise.
    let (mut open, mut head_tip_tree) = (None, None);
    if head_carried && let Some(ht) = head_tip {
        let ht_tree = tree_of(repo, ht)?;
        open = Some(overlay.open_tree(repo, ht_tree)?);
        head_tip_tree = Some(ht_tree);
    }

    // A hold is planned where it would be recorded, and one hold per branch
    // is the rule, so the check sits here rather than at the top of the
    // verb, on purpose: a rewrite that would *succeed* while a hold stands is
    // allowed through, because it is not competing for anything and the hold
    // re-derives itself the next time somebody asks.
    let hold_plan = |at: &At, paths: &[String], of: usize| -> Result<RestackPlan> {
        if overlay.has_hold(&branch) {
            return Err(Error::coded(
                "held/already-held",
                format!("{branch} already has a rewrite held this run: nothing was restacked"),
                vec!["ff resolve".into(), "ff status".into()],
            ));
        }
        held::refuse_if_held(repo, &branch, "restacked")?;
        Ok(RestackPlan::Held(Box::new(HoldPlan {
            branch: branch.clone(),
            base_name: base.name.clone(),
            held: Held {
                intent: Intent::Restack {
                    branch: branch.clone(),
                    onto: base.full.clone(),
                },
                at: at.clone(),
                paths: paths.to_vec(),
                time: now,
            },
            report: HeldReport {
                verb: "restack".into(),
                branch: branch.clone(),
                at: at.clone(),
                paths: paths.to_vec(),
                of,
            },
        })))
    };

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
        // The probe replays the range §5 walked rather than walking its own,
        // so it answers about the commits the plan will rewrite and none of
        // the base's own; §5 already refused a merge in it.
        match futures::probe_range(repo, base_tip, &range, branch_tip, probe_open)? {
            Verdict::Clean { .. } => {
                // Standing mid-stack: the open change belongs to the head
                // branch's tip, not the target's, so it needs its own probe.
                // Replaying up to head_tip is a prefix of the range just
                // proven clean, so only the open-change step can still fail.
                if let (Some(open_t), Some(hb), Some(ht)) = (open, head_branch.as_deref(), head_tip)
                    && hb != branch
                    && let Some(pos) = range.iter().position(|id| *id == ht)
                    && let Verdict::Conflict {
                        at: at @ At::OpenChange,
                        paths,
                    } = futures::probe_range(repo, base_tip, &range[..=pos], ht, Some(open_t))?
                {
                    return hold_plan(&at, &paths, range.len());
                }
            }
            Verdict::Conflict { at, paths } => {
                return hold_plan(&at, &paths, range.len());
            }
            verdict => {
                return Err(Error::msg(format!(
                    "the probe and the range walk disagree ({verdict:?}): internal inconsistency"
                )));
            }
        }
    }

    // 8. Plan, and the new worktree.
    let branch_ref = format!("refs/heads/{branch}");
    let mut carried: Vec<RefTransition> = Vec::new();
    let mut diverged: Vec<String> = Vec::new();
    let mut rewrites: Vec<rewrite::Rewrite> = Vec::new();
    let mut dropped: Vec<rewrite::Dropped> = Vec::new();
    let mut new_tip = branch_tip;
    let mut published = 0usize;
    let mut new_head_tip: Option<gix::ObjectId> = None;
    let mut new_worktree: Option<gix::ObjectId> = None;

    if !up_to_date && !fast_forward {
        // The triple the restack replays — the range §5 walked, which is the
        // one `replan_restack` measures, so the verb and the replan cannot
        // disagree. Read here rather than through `replan_restack` because
        // the tips are the overlay's, not the refs'.
        let Some(target) = range.first().copied() else {
            return Err(Error::msg(format!(
                "{branch} already sits on {base_name}: there is nothing to restack"
            )));
        };
        let plan = rewrite::plan_with(
            repo,
            target,
            branch_tip,
            &rewrite::Change::Onto(base_tip),
            now,
            &decided.trees,
        )?;
        published = rewrite::published_count(repo, &branch, &plan)?;
        // The probe and the plan agree by construction, but only one of them
        // is the thing that ran: the counts come from the plan.
        //
        // `ff restack` moves only the branch it was asked to move: every
        // other local head the rewrite map touched is left where it stood,
        // divergent, rather than carried out from under whatever worktree —
        // this one included — is standing on it. The branch's own transition
        // is built from the tips the plan replayed, since the ref itself may
        // still stand where the run found it.
        diverged = plan
            .carried
            .into_iter()
            .filter(|t| t.name != branch_ref)
            .map(|t| {
                t.name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&t.name)
                    .to_string()
            })
            .collect(); // plan.carried is sorted by ref name, and filter preserves order
        rewrites = plan.rewrites;
        replayed = rewrites.len();
        dropped = plan.dropped;
        new_tip = plan.new_tip;
        carried = vec![RefTransition {
            name: branch_ref.clone(),
            old: Some(branch_tip.to_string()),
            new: Some(new_tip.to_string()),
        }];

        // The head branch's new tip is the branch's own when HEAD is standing
        // on that same branch. When HEAD sits mid-stack on a different,
        // now-divergent branch, its ref is not moving and neither is its
        // worktree.
        if head_carried && head_branch.as_deref() == Some(branch.as_str()) {
            new_head_tip = Some(new_tip);
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
            name: branch_ref.clone(),
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
                return hold_plan(&At::OpenChange, &paths, range.len());
            }
            if open_t == ht_tree {
                new_worktree = Some(ours_tree);
            } else {
                let mut outcome = merge_into(repo, base_tree, ours_tree, open_t)?;
                new_worktree = Some(outcome.tree.write().map_err(Error::repo)?.detach());
            }
        }
    }

    // 9. The branches stacked above. Planned here, once the new tip is known
    // and before anything is written, so the whole cascade rides this
    // operation and one undo takes it back with the restack. HEAD standing
    // on one of them is the one way the working tree moves for a branch it
    // is not on: that branch is carried, so its open change is carried too,
    // and a conflict there holds that branch rather than this verb.
    let cascade = if new_tip != branch_tip {
        let head_above = head_branch
            .as_deref()
            .filter(|h| *h != branch.as_str())
            .map(|branch| cascade::Head { branch });
        cascade::plan_over(repo, &branch, branch_tip, new_tip, head_above, now, overlay)?
    } else {
        CascadePlan::default()
    };
    if let Some(moved) = &cascade.head {
        open = Some(moved.open);
        new_head_tip = Some(moved.new_tip);
        new_worktree = Some(moved.worktree);
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

    Ok(RestackPlan::Replay(Box::new(ReplayPlan {
        branch,
        base,
        branch_tip,
        new_tip,
        bases,
        carried,
        rewrites,
        dropped,
        diverged,
        replayed,
        published,
        fast_forward,
        reaimed,
        parent_changes,
        recorded_parent,
        head_branch,
        head_tip,
        head_carried,
        open,
        new_head_tip,
        new_worktree,
        cascade,
        arrive_plan,
        arrive_target,
    })))
}

/// The committing half of a restack: the operation write-ahead, the refs in
/// one transaction, the recorded parent, the cascade's holds, the worktree,
/// and the report.
fn commit_restack(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: Vec<String>,
    decided: &rewrite::Decided,
    plan: ReplayPlan,
) -> Result<RestackReport> {
    let now = ctx.now;
    let branch = plan.branch.clone();
    let base_name = plan.base.name.clone();

    // 11. Write-ahead: the planned table is the post-restack world.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let mut refs_transitions: Vec<RefTransition> = plan.carried.clone();
    let mut stash_effects: Vec<StashEffect> = Vec::new();
    if !matches!(plan.arrive_plan, ArrivePlan::None) {
        let mut stash_lines: Vec<gix::ObjectId> = refs::read_ref_log(repo, stash::STASH_REF)?
            .iter()
            .map(|l| l.new)
            .collect();
        match &plan.arrive_plan {
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

    let summary = match plan.cascade.report.moved.len() {
        0 => format!("restack {branch} onto {base_name}"),
        n => format!("restack {branch} onto {base_name}, and {n} above it"),
    };
    let mut record = OpRecord::new("restack", summary, now);
    record.argv = argv;
    record.refs = refs_transitions;
    record.stash = stash_effects;
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    if plan.parent_changes {
        record.parent = Some(ParentTransition {
            branch: branch.clone(),
            old: plan.recorded_parent.clone(),
            new: Some(base_name.clone()),
        });
    }
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }

    let mut pins = plan.own_pins()?;

    // The cascade rides this record: its ref moves, rewrites, drops, and
    // holds, and the planned table says where its branches will stand.
    plan.cascade.fold_into(&mut record, &mut planned, &mut pins);

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
            tree: match &plan.arrive_plan {
                ArrivePlan::Restore { target_wip, .. } => *target_wip,
                _ => plan.new_worktree.unwrap_or(ctx.pre_tree),
            },
            // The new tip's tree, so the next foreign `git status` sees the
            // open change against the commit it now sits on.
            index_tree: match &plan.arrive_plan {
                ArrivePlan::Restore { target_index, .. } => *target_index,
                _ => plan
                    .new_head_tip
                    .map(|id| tree_of(repo, id))
                    .transpose()?
                    .unwrap_or(ctx.pre_tree),
            },
            // The chain the op leaves you on: an off-branch restack leaves
            // you exactly where you stood, so HEAD's branch — and only when
            // HEAD is detached or unborn is the restacked one the honest
            // answer.
            branch: plan.head_branch.clone().unwrap_or_else(|| branch.clone()),
            base: Some(plan.branch_tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 12. Mutate.
    // 12.1 Refs: one atomic transaction over every carried head.
    let reflog_msg = format!("restack: onto {base_name}");
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
    // The branches above move in the same transaction: all of them or none.
    edits.extend(plan.cascade.edits(&reflog_msg)?);
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
    if plan.parent_changes {
        let mut meta = branchmeta::read(repo, &branch)?;
        meta.parent = Some(base_name.clone());
        branchmeta::write(repo, &branch, &meta)?;
    }

    // 12.2b The cascade's holds onto their branches, now that the refs have
    // moved, and its futures caches.
    plan.cascade.land(repo)?;

    // 12.3 Worktree: index first, then the files — the order switch.rs uses.
    let mut files = 0usize;
    if let (Some(open_t), Some(new_wt), Some(new_ht)) =
        (plan.open, plan.new_worktree, plan.new_head_tip)
    {
        crate::index::write_index_for_tree(repo, tree_of(repo, new_ht)?)?;
        let everything = |_: &str| true;
        let transition = crate::worktree::apply_tree_transition(repo, open_t, new_wt, &everything)?;
        files = transition.written.len() + transition.deleted.len();
    }

    // 12.3b The parked change comes back, exactly the way `ff switch` brings
    // one back — same plan, same executor, same journal entries.
    let arrival =
        stash::execute_arrival(repo, &branch, &plan.arrive_plan, plan.arrive_target, now)?;

    // 12.4 Futures caches: the restacked branch and every branch it carried.
    // Best-effort — it costs recomputation and nothing else.
    let _ = futures::cache::remove(repo, &branch);
    for t in &plan.carried {
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

    // 13 and 14. The disclosure and the report.
    plan.report(repo, files, Some(&arrival))
}
