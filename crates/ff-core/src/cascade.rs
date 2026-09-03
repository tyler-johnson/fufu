//! The cascade: what a rewrite does to the branches stacked above the one
//! it moved.
//!
//! A branch records the branch it sits on, and following is the whole point
//! of recording it. When a base is rewritten, every local branch whose base
//! resolves to it is replayed onto the new tip, parent before child, through
//! the whole tree. The children of a branch are a scan of branch metadata
//! through [`futures::base_for`]: only `ff start <branch>` records a parent,
//! and a bare start's base is live trunk, so the children of trunk are every
//! local branch that sits on it.
//!
//! The cascade is planned before the triggering operation is written and
//! rides it. The child ref moves, the rewrite map, the drops and the holds
//! all land in the one record, so one `ff undo` takes the whole cascade back
//! together with the rewrite that caused it. This module plans and reports;
//! the ref transaction is the caller's, which is what makes a rewrite and
//! its cascade one atomic move.
//!
//! Four things stop a branch, and each leaves its subtree alone, since a
//! subtree's base did not move. A replay that conflicts holds that branch,
//! recorded on its metadata the way `ff restack` holds, so `ff resolve` on
//! it picks the replay up. A branch checked out in another worktree is
//! skipped and the worktree named: a rewrite must not move a tree out from
//! under whoever is standing in it. A branch already holding a rewrite is
//! skipped, because one hold per branch is the rule. A branch whose range
//! holds a merge is skipped, because replaying a merge is ambiguous.
//!
//! A branch whose commits are all in its base has nothing of its own to
//! replay, and is left where it stands. Replaying the base's own commits
//! onto the base's new tip would drop every one as empty and land the branch
//! on the tip: a move nobody asked for, wearing a report of nothing.
//!
//! The range a child replays is exactly what it holds that its base did
//! not, `base_old..child`, rather than everything above its merge base with
//! the new tip. The difference is the base's own commits. Replayed a second
//! time onto their rewritten selves they drop as empty when the rewrite only
//! moved them and conflict when it changed their content, and neither is
//! news about the child.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::error::{Error, Result};
use crate::futures::{self, Verdict};
use crate::held::{self, Held, Intent};
use crate::model::{Cascade, CascadeHold, CascadeMove, CascadeSkip, HeldReport, SkipReason};
use crate::ops::record::{HeldTransition, OpRecord, RefTransition, RefsTable};
use crate::overlay::Overlay;
use crate::refs;
use crate::rewrite;

/// The stack as branch metadata records it: every local branch, filed under
/// the local branch its base resolves to. Read once per cascade, because
/// `base_for` resolves a trunk and a remote per call and the walk asks for
/// children once per branch it moves.
pub struct Stack {
    children: BTreeMap<String, Vec<String>>,
}

impl Stack {
    pub fn read(repo: &gix::Repository) -> Result<Self> {
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in crate::switch::branch_names(repo)? {
            if let Some(base) = futures::base_for(repo, &name)?
                && let Some(parent) = base.r#ref.strip_prefix("refs/heads/")
                && parent != name
            {
                children.entry(parent.to_string()).or_default().push(name);
            }
        }
        for list in children.values_mut() {
            list.sort();
        }
        Ok(Self { children })
    }

    /// The local branches whose base resolves to `branch`, sorted by name.
    pub fn children(&self, branch: &str) -> &[String] {
        self.children.get(branch).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every branch stacked above `branch`, through the whole tree, sorted.
    /// A parent link `--onto` aimed in a loop ends the walk, not the process.
    pub fn above(&self, branch: &str) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::from([branch.to_string()]);
        let mut queue: VecDeque<String> = self.children(branch).iter().cloned().collect();
        let mut out = Vec::new();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name.clone()) {
                continue;
            }
            queue.extend(self.children(&name).iter().cloned());
            out.push(name);
        }
        out.sort();
        out
    }
}

/// Where HEAD stands, when it stands on a branch other than the one being
/// rewritten. The cascade needs it to know whether one of the branches it
/// moves is the one the working tree is on; the tip is the branch's own.
pub(crate) struct Head<'a> {
    pub branch: &'a str,
}

/// The working tree's move, when HEAD's branch was one the cascade replayed:
/// the open change as it stood, the tip it now sits on, and the tree the
/// worktree will hold once the change is carried onto it.
pub(crate) struct HeadMove {
    pub open: gix::ObjectId,
    pub new_tip: gix::ObjectId,
    pub worktree: gix::ObjectId,
}

/// A planned cascade: the report a verb renders, and everything the verb
/// folds into its own operation to make the cascade ride it.
#[derive(Default)]
pub(crate) struct CascadePlan {
    pub report: Cascade,
    /// One transition per branch that moved.
    pub carried: Vec<RefTransition>,
    pub rewrites: Vec<rewrite::Rewrite>,
    pub dropped: Vec<rewrite::Dropped>,
    /// One per branch that held, for the record and for the metadata.
    pub holds: Vec<HeldTransition>,
    /// The rewritten commits and every old tip, so undo finds them all.
    pub pins: Vec<gix::ObjectId>,
    pub head: Option<HeadMove>,
}

impl CascadePlan {
    /// Fold the cascade into the triggering operation's record, planned ref
    /// table, and pins, so it rides that operation and one undo takes both
    /// back.
    pub(crate) fn fold_into(
        &self,
        record: &mut OpRecord,
        planned: &mut RefsTable,
        pins: &mut Vec<gix::ObjectId>,
    ) {
        record.refs.extend(self.carried.iter().cloned());
        record.rewrites.extend(self.rewrites.iter().cloned());
        record.dropped.extend(self.dropped.iter().cloned());
        record.cascade_held.extend(self.holds.iter().cloned());
        for t in &self.carried {
            if let Some(new) = &t.new {
                planned.refs.insert(t.name.clone(), new.clone());
            }
        }
        pins.extend(self.pins.iter().copied());
    }

    /// The ref edits, for the caller's one transaction: every child moves
    /// with the branch beneath it or none of them do.
    pub(crate) fn edits(&self, reflog_msg: &str) -> Result<Vec<gix::refs::transaction::RefEdit>> {
        let mut edits = Vec::new();
        for t in &self.carried {
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
        Ok(edits)
    }

    /// After the refs have moved: the holds onto their branches' metadata,
    /// and the futures caches of every branch that moved, which cost
    /// recomputation and nothing else.
    pub(crate) fn land(&self, repo: &gix::Repository) -> Result<()> {
        for h in &self.holds {
            held::set(repo, &h.branch, h.new.clone())?;
        }
        for t in &self.carried {
            if let Some(name) = t.name.strip_prefix("refs/heads/") {
                let _ = futures::cache::remove(repo, name);
            }
        }
        Ok(())
    }
}

/// One branch waiting its turn: the branch, the base it sits on, and where
/// that base stood before and after its move.
struct Step {
    branch: String,
    base: String,
    base_old: gix::ObjectId,
    base_new: gix::ObjectId,
}

/// Plan the replay of every branch stacked above `root`, which has moved
/// from `root_old` to `root_new`. Writes commit objects and moves no ref:
/// the caller folds the plan into its operation and lands it.
pub(crate) fn plan(
    repo: &gix::Repository,
    root: &str,
    root_old: gix::ObjectId,
    root_new: gix::ObjectId,
    head: Option<Head<'_>>,
    now: i64,
) -> Result<CascadePlan> {
    plan_over(
        repo,
        root,
        root_old,
        root_new,
        head,
        now,
        &Overlay::default(),
    )
}

/// [`plan`], reading the branches' tips, their holds, and the working tree
/// through what a run has already planned and not written. `ff sync` plans
/// every branch's axes against one overlay and writes one operation; a
/// verb with nothing planned ahead passes an empty one, which reads the
/// repository as it stands.
pub(crate) fn plan_over(
    repo: &gix::Repository,
    root: &str,
    root_old: gix::ObjectId,
    root_new: gix::ObjectId,
    head: Option<Head<'_>>,
    now: i64,
    overlay: &Overlay,
) -> Result<CascadePlan> {
    let stack = Stack::read(repo)?;
    let holders = crate::linked::holders(repo)?;
    let mut out = CascadePlan::default();
    // The root is visited before anything else is, so a parent link aimed
    // back at it ends the walk.
    let mut visited: HashSet<String> = HashSet::from([root.to_string()]);
    let mut queue: VecDeque<Step> = VecDeque::new();
    for child in stack.children(root) {
        queue.push_back(Step {
            branch: child.clone(),
            base: root.to_string(),
            base_old: root_old,
            base_new: root_new,
        });
    }

    while let Some(Step {
        branch,
        base,
        base_old,
        base_new,
    }) = queue.pop_front()
    {
        if !visited.insert(branch.clone()) {
            continue;
        }
        let left_alone = stack.above(&branch);

        if let Some(holder) = holders.iter().find(|h| h.branch == branch) {
            out.report.skipped.push(CascadeSkip {
                branch,
                base,
                reason: SkipReason::Worktree {
                    path: holder.path.display().to_string(),
                },
                left_alone,
            });
            continue;
        }
        if overlay.held(repo, &branch)?.is_some() {
            out.report.skipped.push(CascadeSkip {
                branch,
                base,
                reason: SkipReason::AlreadyHeld,
                left_alone,
            });
            continue;
        }
        let Some(tip) = overlay.branch_tip(repo, &branch)? else {
            continue;
        };

        let bases: Vec<gix::ObjectId> = repo
            .merge_bases_many(tip, &[base_new])
            .map_err(Error::repo)?
            .into_iter()
            .map(|id| id.detach())
            .collect();
        if bases.is_empty() {
            out.report.skipped.push(CascadeSkip {
                branch,
                base,
                reason: SkipReason::Unrelated,
                left_alone,
            });
            continue;
        }
        // Already on the new tip, or wholly inside it: nothing of its own.
        if bases.contains(&base_new) || bases.contains(&tip) {
            out.report.unchanged.push(branch);
            continue;
        }
        // Wholly inside the base as it stood — at its old tip, or partway
        // up it: nothing of its own either.
        let old_bases: Vec<gix::ObjectId> = repo
            .merge_bases_many(tip, &[base_old])
            .map_err(Error::repo)?
            .into_iter()
            .map(|id| id.detach())
            .collect();
        if old_bases.contains(&tip) {
            out.report.unchanged.push(branch);
            continue;
        }

        // What the branch holds that its base did not: bounded by where it
        // forked from the base as it stood, so the base's own commits are
        // not replayed twice. A boundary stops the walk where it is reached
        // rather than hiding what it can reach, which is why the fork point
        // is the bound and not the old tip.
        let mut boundary = bases;
        boundary.extend(old_bases);
        let walk = repo
            .rev_walk(Some(tip))
            .with_boundary(boundary)
            .all()
            .map_err(Error::repo)?;
        let mut range: Vec<gix::ObjectId> = Vec::new();
        let mut merge = false;
        for info in walk {
            let info = info.map_err(Error::repo)?;
            if info.parent_ids().count() > 1 {
                merge = true;
                break;
            }
            range.push(info.id);
        }
        if merge {
            out.report.skipped.push(CascadeSkip {
                branch,
                base,
                reason: SkipReason::MergeInRange,
                left_alone,
            });
            continue;
        }
        if range.is_empty() {
            out.report.unchanged.push(branch);
            continue;
        }
        range.reverse(); // oldest-first; the target is the first element

        // The open change comes along when HEAD stands here, and is probed
        // as the last step exactly as the branch underfoot's is.
        let here = head.as_ref().is_some_and(|h| h.branch == branch);
        let (open, tip_tree) = if here {
            let tip_tree = futures::tree_of(repo, tip)?;
            (Some(overlay.open_tree(repo, tip_tree)?), Some(tip_tree))
        } else {
            (None, None)
        };

        match futures::probe_range(repo, base_new, &range, tip, open)? {
            Verdict::Clean { .. } => {}
            Verdict::Conflict { at, paths } => {
                let held = Held {
                    intent: Intent::Restack {
                        branch: branch.clone(),
                        onto: format!("refs/heads/{base}"),
                    },
                    at: at.clone(),
                    paths: paths.clone(),
                    time: now,
                };
                out.holds.push(HeldTransition {
                    branch: branch.clone(),
                    old: None,
                    new: Some(held),
                });
                out.report.held.push(CascadeHold {
                    branch: branch.clone(),
                    base,
                    report: HeldReport {
                        verb: "restack".into(),
                        branch,
                        at,
                        paths,
                        of: range.len(),
                    },
                    left_alone,
                });
                continue;
            }
            verdict => {
                return Err(Error::msg(format!(
                    "the cascade's probe and its range walk disagree ({verdict:?}): internal \
                     inconsistency"
                )));
            }
        }

        let plan = rewrite::plan_with(
            repo,
            range[0],
            tip,
            &rewrite::Change::Onto(base_new),
            now,
            &HashMap::new(),
        )?;
        let published = rewrite::published_count(repo, &branch, &plan)?;
        let published_on = rewrite::tracking_name(repo, &branch)?;
        let own_ref = format!("refs/heads/{branch}");
        // Other heads inside this branch's range stay where they stood, the
        // rule `ff restack` applies to the branch it was asked to move.
        let diverged: Vec<String> = plan
            .carried
            .iter()
            .filter(|t| t.name != own_ref)
            .map(|t| {
                t.name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&t.name)
                    .to_string()
            })
            .collect();
        let new_tip = plan.new_tip;

        if let (Some(open), Some(tip_tree)) = (open, tip_tree) {
            let new_tip_tree = futures::tree_of(repo, new_tip)?;
            let worktree = if open == tip_tree {
                new_tip_tree
            } else {
                let options = repo.tree_merge_options().map_err(Error::repo)?;
                let mut outcome = repo
                    .merge_trees(tip_tree, new_tip_tree, open, Default::default(), options)
                    .map_err(Error::repo)?;
                outcome.tree.write().map_err(Error::repo)?.detach()
            };
            out.head = Some(HeadMove {
                open,
                new_tip,
                worktree,
            });
        }

        for r in &plan.rewrites {
            out.pins
                .push(gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo)?);
        }
        out.pins.push(tip);
        out.carried.push(RefTransition {
            name: own_ref,
            old: Some(tip.to_string()),
            new: Some(new_tip.to_string()),
        });
        out.rewrites.extend(plan.rewrites.iter().cloned());
        out.dropped.extend(plan.dropped.iter().cloned());
        for child in stack.children(&branch) {
            queue.push_back(Step {
                branch: child.clone(),
                base: branch.clone(),
                base_old: tip,
                base_new: new_tip,
            });
        }
        out.report.moved.push(CascadeMove {
            branch,
            base,
            old_tip: tip.to_string(),
            new_tip: new_tip.to_string(),
            replayed: plan.rewrites.len(),
            dropped: plan.dropped,
            diverged,
            published,
            published_on,
        });
    }

    Ok(out)
}
