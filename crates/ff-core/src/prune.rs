//! `ff branch --prune`: delete every local branch whose shared copy is gone,
//! in one operation, and the classification `ff pull` borrows under
//! `fufu.pruneGone`.
//!
//! Gone has three parts, all required: the branch has an upstream
//! configured, its tracking ref is absent, and there is a record that the
//! copy once stood — `refs/fufu/seen/<branch>` or
//! `refs/fufu/published/<branch>`. Without the record the shape is the
//! fresh clone's, a copy never created, and the branch is unpublished, not
//! gone. An upstream under another branch's name is the base the branch was
//! cut from, not a copy, and never gone.
//!
//! A gone branch is kept, and named, for four reasons: it is the branch
//! underfoot; another worktree has it checked out; a rewrite is held on it;
//! or its tip holds commits the copy never held, work the copy's deletion
//! did not take. There is no `--force`: `ff branch -d <name>` is the verb
//! for deleting one branch on purpose, and it is already undoable.
//!
//! The mechanics of each delete are [`crate::branch::DeletePlan`]'s, the
//! same ones `ff branch -d` applies, and the branches stacked on a pruned
//! one are re-aimed the way `ff fold` re-aims them: each child's recorded
//! parent becomes what the pruned branch sat on, resolved through the whole
//! set of deletes so a stack whose bottom landed sits on what the bottom
//! sat on. One operation, so one `ff undo` brings every branch back with
//! its timeline, its parent link, and its config section — the tracking
//! section and the seen and published refs are left standing exactly as a
//! plain `-d` leaves them, since they are what make the branch whole when
//! it returns.

use std::collections::{BTreeMap, HashSet};

use crate::branch::{DeletePlan, plan_delete};
use crate::error::Result;
use crate::model::{BranchPruneReport, Kept, KeptBranch, PrunedBranch, Reaim};
use crate::ops::record::{ParentTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, verb};
use crate::preflight::Verb;

/// What a prune would do: the deletes, the branches it keeps and why, and
/// the parent links it rewrites on the branches stacked above a delete.
#[derive(Debug, Clone)]
pub struct PrunePlan {
    pub deletes: Vec<DeletePlan>,
    pub kept: Vec<KeptBranch>,
    pub reaims: Vec<ParentTransition>,
}

impl PrunePlan {
    /// The re-aims of one deleted branch's children, as the report says
    /// them.
    pub fn reaimed(&self, repo: &gix::Repository, deleted: &str) -> Vec<Reaim> {
        reaimed(repo, &self.reaims, deleted)
    }
}

/// The re-aims among `reaims` whose old parent is `deleted`, as the report
/// says them: the branch and what it now sits on. A child whose new parent
/// is unrecorded sits on trunk, which is what it will read as its base, so
/// trunk's name is what the person is told.
pub(crate) fn reaimed(
    repo: &gix::Repository,
    reaims: &[ParentTransition],
    deleted: &str,
) -> Vec<Reaim> {
    let trunk = crate::trunk::trunk(repo).ok().map(|t| t.name);
    reaims
        .iter()
        .filter(|t| t.old.as_deref() == Some(deleted))
        .map(|t| Reaim {
            branch: t.branch.clone(),
            onto: t.new.clone().or_else(|| trunk.clone()),
        })
        .collect()
}

/// Whether `name`'s shared copy is gone: an upstream of its own configured,
/// its tracking ref absent, and a seen or published record that the copy
/// once stood.
pub fn is_gone(repo: &gix::Repository, name: &str) -> Result<bool> {
    let Some(copy) = crate::futures::remote_for(repo, name)? else {
        return Ok(false);
    };
    if !copy.tip.is_empty() {
        return Ok(false);
    }
    Ok(record_of(repo, name)?.is_some())
}

/// The tip the copy last stood at as far as fufu knows: seen first, since
/// it is the later fact, then published.
fn record_of(repo: &gix::Repository, name: &str) -> Result<Option<gix::ObjectId>> {
    Ok(match crate::seen::last_seen(repo, name)? {
        Some(id) => Some(id),
        None => crate::published::last_published(repo, name)?,
    })
}

/// Classify every local branch whose shared copy is gone: a delete, or kept
/// with its reason. `current` is the branch underfoot, which is kept.
pub fn plan(repo: &gix::Repository, current: &str) -> Result<PrunePlan> {
    let mut names = crate::switch::branch_names(repo)?;
    names.sort();
    let holders = crate::linked::holders(repo)?;
    let mut deletes = Vec::new();
    let mut kept = Vec::new();
    for name in names {
        if !is_gone(repo, &name)? {
            continue;
        }
        let reason = if name == current {
            Some(Kept::Current)
        } else if let Some(holder) = holders.iter().find(|h| h.branch == name) {
            Some(Kept::Elsewhere {
                path: holder.path.display().to_string(),
            })
        } else if let Some(held) = crate::held::of(repo, &name)? {
            Some(Kept::Held {
                verb: crate::held::verb_of(&held),
            })
        } else {
            let plan = plan_delete(repo, &name)?;
            match ahead(repo, &name, plan.tip)? {
                0 => {
                    deletes.push(plan);
                    None
                }
                count => Some(Kept::Ahead { count }),
            }
        };
        if let Some(reason) = reason {
            kept.push(KeptBranch { name, reason });
        }
    }

    // The re-aims: every branch stacked on a delete lands on what the
    // deleted branch sat on, resolved through the whole set so a run of
    // deletes through a stack leaves its top on what the bottom stood on.
    let deleted: BTreeMap<String, Option<String>> = deletes
        .iter()
        .map(|d| {
            Ok((
                d.name.clone(),
                crate::branchmeta::read(repo, &d.name)?.parent,
            ))
        })
        .collect::<Result<_>>()?;
    let stack = crate::cascade::Stack::read(repo)?;
    let mut reaims = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in deleted.keys() {
        for child in stack.children(name) {
            if deleted.contains_key(child) || !seen.insert(child.clone()) {
                continue;
            }
            let old = crate::branchmeta::read(repo, child)?.parent;
            let new = surviving_parent(&deleted, name);
            if old == new {
                continue;
            }
            reaims.push(ParentTransition {
                branch: child.clone(),
                old,
                new,
            });
        }
    }
    Ok(PrunePlan {
        deletes,
        kept,
        reaims,
    })
}

/// The first recorded parent above `name` that is not itself being deleted,
/// or `None` when the chain ends on nothing. A parent link aimed in a loop
/// through the deletes ends the walk at the first branch seen twice.
fn surviving_parent(deleted: &BTreeMap<String, Option<String>>, name: &str) -> Option<String> {
    let mut walked: HashSet<&str> = HashSet::new();
    let mut at = name;
    loop {
        if !walked.insert(at) {
            return None;
        }
        match deleted.get(at) {
            Some(Some(parent)) => at = parent,
            Some(None) => return None,
            None => return Some(at.to_string()),
        }
    }
}

/// How many commits `tip` holds that the copy never did, measured against
/// the record of where it last stood. A record whose commit is unreadable
/// counts every commit as ahead: a branch is never deleted on a guess.
fn ahead(repo: &gix::Repository, name: &str, tip: gix::ObjectId) -> Result<usize> {
    let Some(record) = record_of(repo, name)? else {
        // Unreachable through `is_gone`, and the safe answer regardless.
        return crate::upstream::count_exclusive(repo, tip, &[]);
    };
    if !matches!(repo.try_find_object(record), Ok(Some(_))) {
        return crate::upstream::count_exclusive(repo, tip, &[]);
    }
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(tip, &[record])
        .map_err(crate::Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    crate::upstream::count_exclusive(repo, tip, &bases)
}

pub struct PruneOptions {
    /// Plan and report, and write nothing.
    pub dry_run: bool,
    /// A fetch ran this invocation; the report carries it.
    pub fetched: bool,
}

/// `ff branch --prune`: one operation deleting every branch [`plan`] found
/// gone and unkept. Under `dry_run` the context is `None`, since no capture
/// was taken and nothing was written; a run that finds nothing to delete
/// writes nothing either.
pub fn prune(
    repo: &gix::Repository,
    opts: PruneOptions,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(BranchPruneReport, Option<verb::VerbContext>)> {
    let current = crate::preflight::head_branch(repo, Verb::Prune)?;
    let ctx = if opts.dry_run {
        None
    } else {
        Some(verb::begin_verb(repo, prov, now)?)
    };
    let now = ctx
        .as_ref()
        .map_or_else(|| verb::now_or_wall_clock(now), |c| c.now);
    let plan = plan(repo, &current)?;
    let pre_op = ctx.as_ref().and_then(|c| c.pre_op.map(|id| id.to_string()));

    let mut pruned: Vec<PrunedBranch> = plan
        .deletes
        .iter()
        .map(|d| PrunedBranch {
            name: d.name.clone(),
            tip: d.tip.to_string(),
            open_left: d.open_left.map(|id| id.to_string()),
            trash_ref: None,
            reaimed: plan.reaimed(repo, &d.name),
        })
        .collect();

    // Nothing to delete, or a dry run: no operation. The real run's
    // capture stands either way, so its reconcile notice is still said.
    if opts.dry_run || plan.deletes.is_empty() {
        // Under a dry run the trash ref is where the pointer would go.
        for (p, d) in pruned.iter_mut().zip(plan.deletes.iter()) {
            p.trash_ref = d.snap_tip.map(|_| format!("refs/fufu/trash/{}", d.name));
        }
        return Ok((
            BranchPruneReport {
                pruned,
                kept: plan.kept,
                fetched: opts.fetched,
                dry_run: opts.dry_run,
                pre_op,
            },
            ctx,
        ));
    }
    let ctx = ctx.expect("a real run has its context");

    // The one operation: every delete's transitions and pointer, every
    // re-aim, written ahead of the first ref move.
    let head = crate::head::head_state(repo)?;
    let mut planned = observe_refs(repo)?;
    let mut record = OpRecord::new(
        "branch",
        format!(
            "prune {} branch(es) whose shared copy is gone",
            plan.deletes.len()
        ),
        now,
    );
    record.argv = argv;
    let mut pins = Vec::new();
    for d in &plan.deletes {
        d.remove_from(&mut planned);
        record.refs.extend(d.transitions());
        record.pointers.extend(d.pointer());
        pins.extend(d.pins());
    }
    record.inferred_parents = plan.reaims.clone();
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Deleting other branches leaves this worktree untouched.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: current.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    for (p, d) in pruned.iter_mut().zip(plan.deletes.iter()) {
        p.trash_ref = d.apply(repo, now)?;
    }
    for t in &plan.reaims {
        let mut meta = crate::branchmeta::read(repo, &t.branch)?;
        meta.parent = t.new.clone();
        crate::branchmeta::write(repo, &t.branch, &meta)?;
    }

    Ok((
        BranchPruneReport {
            pruned,
            kept: plan.kept,
            fetched: opts.fetched,
            dry_run: false,
            pre_op,
        },
        Some(ctx),
    ))
}
