//! The park is the open commit, and this is the arrival.
//!
//! Leaving a branch writes nothing: the pre-verb capture already wrote the
//! open change as a commit at `refs/fufu/open/<branch>` — the tree over the
//! branch's tip, the change id as a header — and the operation that leaves
//! pins it. Arriving lays that commit's tree back over the tip when the tip
//! is still its parent; when the tip moved, the commit is replayed onto it
//! as one three-way merge and written again with the same id; when the
//! replay conflicts, the branch is held for `ff resolve` and the commit
//! stays pinned by the operation; when the replay is empty, the tip already
//! holds the change and its id goes.
//!
//! A park made before this shape — a `git stash push -u`-shaped entry plus
//! `refs/fufu/parked/<branch>` — folds into an open commit on the first
//! arrival and is spent, so no verb has to know two shapes for long. That
//! reader lives in [`crate::stash`].
//!
//! Every verb that lands somewhere plans its arrival here before it moves
//! anything, folds the plan into its write-ahead record, and executes it
//! after its refs have moved, so one `ff undo` takes the landing and the
//! arrival back together.

use crate::branchmeta::{self, BranchMeta};
use crate::changeid::{self, ChangeId};
use crate::error::{Error, Result};
use crate::futures;
use crate::held::{Held, Intent};
use crate::model::ArrivalReport;
use crate::open;
use crate::ops::OpRecord;
use crate::ops::record::{
    ChangeIdTransition, DescriptionTransition, HeldTransition, RefTransition, RefsTable,
};
use crate::refs;
use crate::stash;
use crate::worktree;

/// What the arrival does with the parked change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrive {
    /// Nothing is parked on the branch.
    None,
    /// The open commit's parent is the tip: its tree is laid back.
    Resume {
        open: gix::ObjectId,
        tree: gix::ObjectId,
    },
    /// The tip moved: `from` replayed onto it as `open`, same id, written
    /// here with the verb's clock.
    Replay {
        from: gix::ObjectId,
        open: gix::ObjectId,
        tree: gix::ObjectId,
    },
    /// The replay conflicts: the branch is held, and `open` stays pinned by
    /// the operation.
    Hold {
        open: gix::ObjectId,
        paths: Vec<String>,
    },
    /// The replay is empty — the tip already holds the change. The id goes.
    Landed { open: gix::ObjectId },
    /// Legacy: the stash entry was dropped outside fufu. The id goes.
    Invalidate { stash: gix::ObjectId },
}

/// An arrival, planned before anything moves.
#[derive(Debug, Clone)]
pub struct ArrivePlan {
    pub branch: String,
    pub what: Arrive,
    /// A legacy stash entry this arrival spends: folded into the open commit
    /// the plan goes on to arrive with.
    pub folded: Option<gix::ObjectId>,
    /// A change id minted for a legacy park on a branch that had none, with
    /// its birth. Recorded on the operation, written after it.
    minted: Option<(String, i64)>,
    /// The branch's metadata as the plan read it: what a hold or a landing
    /// takes off the branch, so the record can say what stood there.
    meta: BranchMeta,
    now: i64,
}

impl ArrivePlan {
    /// No arrival: the verb lands nowhere a park could wait.
    pub fn none() -> Self {
        ArrivePlan {
            branch: String::new(),
            what: Arrive::None,
            folded: None,
            minted: None,
            meta: BranchMeta::default(),
            now: 0,
        }
    }

    /// The open commit the arrival leaves on the branch, for
    /// [`crate::ops::verb::append_op_hinted`]: the operation lands it rather
    /// than minting a twin.
    pub fn open_hint(&self) -> Option<gix::ObjectId> {
        match &self.what {
            Arrive::Resume { open, .. } | Arrive::Replay { open, .. } => Some(*open),
            Arrive::None
            | Arrive::Hold { .. }
            | Arrive::Landed { .. }
            | Arrive::Invalidate { .. } => None,
        }
    }

    /// The trees a verb records as its end state: the restored change laid
    /// over the target when the arrival brings one back — the working copy
    /// holds it, and the record must say so or the next undo reads it as
    /// drift — else the target's tree on both axes. The index is always the
    /// target's: a park carries the tree, not the index.
    pub fn end_trees(&self, target_tree: gix::ObjectId) -> (gix::ObjectId, gix::ObjectId) {
        match &self.what {
            Arrive::Resume { tree, .. } | Arrive::Replay { tree, .. } => (*tree, target_tree),
            _ => (target_tree, target_tree),
        }
    }

    /// Fold the arrival into the verb's write-ahead: the pins, a hold and
    /// the identity it takes off the branch, an id dropped or minted, and
    /// the legacy bookkeeping for an entry the arrival spends. `stash_lines`
    /// is the stash reflog as the verb read it, for the legacy half.
    pub fn fold_into(
        &self,
        planned: &mut RefsTable,
        record: &mut OpRecord,
        pins: &mut Vec<gix::ObjectId>,
        stash_lines: &mut Vec<gix::ObjectId>,
    ) {
        let branch = &self.branch;
        // What the branch wore before the arrival. A minted id is written
        // after the append and never stood on the branch, so an arrival that
        // drops the id records the branch's own, not the mint.
        let before = (self.meta.change_id.clone(), self.meta.change_born);
        match &self.what {
            Arrive::None => {}
            Arrive::Resume { open, .. } => pins.push(*open),
            Arrive::Replay { from, open, .. } => {
                pins.push(*from);
                pins.push(*open);
            }
            Arrive::Hold { open, paths } => {
                pins.push(*open);
                let held = Held {
                    intent: Intent::Arrive {
                        branch: branch.clone(),
                        open: open.to_string(),
                    },
                    at: futures::At::OpenChange,
                    paths: paths.clone(),
                    time: self.now,
                };
                // The hold takes the place of one already standing: a hold is a
                // cache over "this conflicts", and the verb that recorded the
                // old one re-records it when re-run. Recorded, so undo puts the
                // old one back.
                match &mut record.held {
                    Some(t) if t.branch == *branch => t.new = Some(held),
                    Some(_) => record.cascade_held.push(HeldTransition {
                        branch: branch.clone(),
                        old: self.meta.held.clone(),
                        new: Some(held),
                    }),
                    None => {
                        record.held = Some(HeldTransition {
                            branch: branch.clone(),
                            old: self.meta.held.clone(),
                            new: Some(held),
                        });
                    }
                }
                // The identity travels with the held commit.
                change_id_transition(record, branch, before.clone(), (None, None));
                if let Some(old) = &self.meta.pending_description {
                    record.description = Some(DescriptionTransition {
                        branch: branch.clone(),
                        old: Some(old.clone()),
                        new: None,
                    });
                }
            }
            Arrive::Landed { open } => {
                pins.push(*open);
                change_id_transition(record, branch, before.clone(), (None, None));
            }
            Arrive::Invalidate { stash } => {
                planned.refs.remove(&stash::parked_ref(branch));
                record.refs.push(RefTransition {
                    name: stash::parked_ref(branch),
                    old: Some(stash.to_string()),
                    new: None,
                });
                pins.push(*stash);
                change_id_transition(record, branch, before.clone(), (None, None));
            }
        }
        if let Some(stash) = self.folded {
            stash::spend(stash_lines, planned, record, pins, branch, stash);
        }
        if let Some((id, born)) = &self.minted
            && matches!(self.what, Arrive::Resume { .. } | Arrive::Replay { .. })
        {
            change_id_transition(record, branch, before, (Some(id.clone()), Some(*born)));
        }
    }
}

/// Record an id transition on `branch`, merged into one the record already
/// carries for it — an abandon clearing a hold whose arrival holds again
/// is one transition, old to new — and skipped when it changes nothing.
fn change_id_transition(
    record: &mut OpRecord,
    branch: &str,
    old: (Option<String>, Option<i64>),
    new: (Option<String>, Option<i64>),
) {
    match &mut record.change_id {
        Some(t) if t.branch == branch => {
            t.new = new.0;
            t.new_born = new.1;
            if t.old == t.new && t.old_born == t.new_born {
                record.change_id = None;
            }
        }
        _ => {
            if old == new {
                return;
            }
            record.change_id = Some(ChangeIdTransition {
                branch: branch.to_string(),
                old: old.0,
                new: new.0,
                old_born: old.1,
                new_born: new.1,
            });
        }
    }
}

fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_commit(commit)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The tree and first parent of a commit.
fn anatomy(
    repo: &gix::Repository,
    commit: gix::ObjectId,
) -> Result<(gix::ObjectId, Option<gix::ObjectId>)> {
    let commit = repo.find_commit(commit).map_err(Error::repo)?;
    let tree = commit.tree_id().map_err(Error::repo)?.detach();
    let parent = commit.parent_ids().next().map(|p| p.detach());
    Ok((tree, parent))
}

/// The tree a commit's first parent carries: the empty tree for a root.
fn parent_tree(repo: &gix::Repository, parent: Option<gix::ObjectId>) -> Result<gix::ObjectId> {
    match parent {
        Some(p) => tree_of(repo, p),
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// Plan the arrival on `branch` against the tip the verb lands it on, before
/// HEAD moves. Writes objects only — a replayed open commit — never refs.
pub fn plan_arrival(
    repo: &gix::Repository,
    branch: &str,
    target_commit: gix::ObjectId,
    target_tree: gix::ObjectId,
    now: i64,
) -> Result<ArrivePlan> {
    let meta = branchmeta::read(repo, branch)?;
    let mut plan = ArrivePlan {
        branch: branch.to_string(),
        what: Arrive::None,
        folded: None,
        minted: None,
        meta,
        now,
    };

    // Legacy first: a park made before the open commit was the park. Its
    // entry can have been dropped outside fufu, which demotes it as it
    // always did; otherwise it folds into an open commit here, and the
    // arrival goes on from that commit as from any other.
    let open = match stash::parked_entry(repo, branch)? {
        Some(stash) => {
            if !stash::stash_contains(repo, stash)? {
                plan.what = Arrive::Invalidate { stash };
                return Ok(plan);
            }
            let (id, born) = match (&plan.meta.change_id, plan.meta.change_born) {
                (Some(id), born) => (id.clone(), born.unwrap_or(now)),
                (None, _) => {
                    let id = ChangeId::mint()?.letters();
                    plan.minted = Some((id.clone(), now));
                    (id, now)
                }
            };
            let commit = fold_legacy(
                repo,
                branch,
                stash,
                &id,
                born,
                plan.meta.pending_description.as_deref(),
                now,
            )?;
            plan.folded = Some(stash);
            Some(commit)
        }
        None => refs::ref_target(repo, &open::open_ref(branch))?,
    };
    let Some(open) = open else {
        return Ok(plan);
    };
    let overlay = identity_overlay(repo, &plan.meta, plan.minted.as_ref(), open)?;
    plan.what = replay(
        repo,
        branch,
        open,
        target_commit,
        target_tree,
        &overlay,
        now,
    )?;
    Ok(plan)
}

/// Plan the arrival of one open commit the verb already holds — a held
/// arrival being resolved, whose commit the hold names rather than the
/// branch's ref.
pub fn plan_replay(
    repo: &gix::Repository,
    branch: &str,
    open: gix::ObjectId,
    target_commit: gix::ObjectId,
    target_tree: gix::ObjectId,
    now: i64,
) -> Result<ArrivePlan> {
    let meta = branchmeta::read(repo, branch)?;
    let overlay = identity_overlay(repo, &meta, None, open)?;
    let what = replay(
        repo,
        branch,
        open,
        target_commit,
        target_tree,
        &overlay,
        now,
    )?;
    Ok(ArrivePlan {
        branch: branch.to_string(),
        what,
        folded: None,
        minted: None,
        meta,
        now,
    })
}

/// The identity a replayed open commit wears: the branch's, and the open
/// commit's own header and author time where the branch has none — the
/// header is what the commit was written from.
fn identity_overlay(
    repo: &gix::Repository,
    meta: &BranchMeta,
    minted: Option<&(String, i64)>,
    open: gix::ObjectId,
) -> Result<open::Overlay> {
    let (id, born) = match minted {
        Some((id, born)) => (Some(id.clone()), Some(*born)),
        None => (meta.change_id.clone(), meta.change_born),
    };
    let commit = repo.find_commit(open).map_err(Error::repo)?;
    let id = match id {
        Some(id) => Some(id),
        None => changeid::header_of(&commit.data).map(|id| id.letters()),
    };
    let born = match born {
        Some(born) => Some(born),
        None => Some(
            commit
                .author()
                .map_err(Error::repo)?
                .time()
                .map_err(Error::repo)?
                .seconds,
        ),
    };
    Ok(open::Overlay {
        description: None,
        change_id: Some((id, born)),
    })
}

/// The arrival of `open` on a tip: resume, replay, hold, or landed.
fn replay(
    repo: &gix::Repository,
    branch: &str,
    open: gix::ObjectId,
    target_commit: gix::ObjectId,
    target_tree: gix::ObjectId,
    overlay: &open::Overlay,
    now: i64,
) -> Result<Arrive> {
    let (tree, parent) = anatomy(repo, open)?;
    if parent == Some(target_commit) {
        return Ok(Arrive::Resume { open, tree });
    }
    let base = parent_tree(repo, parent)?;
    let paths = futures::conflict_paths(repo, base, target_tree, tree)?;
    if !paths.is_empty() {
        return Ok(Arrive::Hold { open, paths });
    }
    let merged = merge_tree(repo, base, target_tree, tree)?;
    if merged == target_tree {
        return Ok(Arrive::Landed { open });
    }
    let plan =
        open::plan(repo, branch, merged, Some(target_commit), overlay, now)?.ok_or_else(|| {
            Error::msg(format!(
                "the parked change on {branch} cannot be replayed onto the branch's new tip: set \
                 user.name and user.email so the replay can write an open commit"
            ))
        })?;
    let new = open::write(repo, &plan, &[], now)?;
    Ok(Arrive::Replay {
        from: open,
        open: new,
        tree: merged,
    })
}

/// A clean three-way merge of trees, written to the object store. Asked only
/// after [`futures::conflict_paths`] said it is clean, so nothing here is a
/// probe.
fn merge_tree(
    repo: &gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<gix::ObjectId> {
    let options = repo
        .tree_merge_options()
        .map_err(Error::repo)?
        .with_fail_on_conflict(Some(gix::merge::tree::TreatAsUnresolved::git()));
    let mut outcome = repo
        .merge_trees(base, ours, theirs, Default::default(), options)
        .map_err(Error::repo)?;
    if outcome.failed_on_first_unresolved_conflict {
        return Err(Error::msg(
            "internal: a replay the probe called clean conflicted when written",
        ));
    }
    Ok(outcome.tree.write().map_err(Error::repo)?.detach())
}

/// Fold a legacy stash entry into an open commit: its wip tree with its
/// untracked files laid over, on the base it was taken on, wearing the
/// branch's identity. The branch's open ref is reused when it already says
/// exactly that — a pre-flight park left one beside the entry.
fn fold_legacy(
    repo: &gix::Repository,
    branch: &str,
    stash: gix::ObjectId,
    id: &str,
    born: i64,
    description: Option<&str>,
    now: i64,
) -> Result<gix::ObjectId> {
    let entry = stash::read_stash_commit(repo, stash)?;
    let tree = if entry.untracked_tree == gix::ObjectId::empty_tree(repo.object_hash()) {
        entry.wip_tree
    } else {
        let mut editor = repo.edit_tree(entry.wip_tree).map_err(Error::repo)?;
        for (path, kind, id) in stash::tree_files(repo, entry.untracked_tree)? {
            editor
                .upsert(path.as_str(), kind, id)
                .map_err(Error::repo)?;
        }
        editor.write().map_err(Error::repo)?.detach()
    };
    let change_id = ChangeId::parse(id).ok_or_else(|| {
        Error::msg(format!(
            "corrupt branch metadata for {branch}: change id {id:?} is not one"
        ))
    })?;
    let plan = open::OpenPlan {
        tree,
        parent: Some(entry.base),
        change_id,
        born,
        message: crate::close::normalize_message(description.unwrap_or("")),
    };
    let existing: Vec<gix::ObjectId> = refs::ref_target(repo, &open::open_ref(branch))?
        .into_iter()
        .collect();
    open::write(repo, &plan, &existing, now).map_err(|err| {
        Error::msg(format!(
            "the park on {branch} cannot fold into an open commit: {err}"
        ))
    })
}

/// The mutating half of an arrival, after the verb's refs have moved and the
/// worktree holds `target_tree`: the tree laid back, a hold recorded, an id
/// dropped, the legacy entry spent.
pub fn execute_arrival(
    repo: &gix::Repository,
    branch: &str,
    plan: &ArrivePlan,
    target_tree: gix::ObjectId,
    now: i64,
) -> Result<ArrivalReport> {
    if let Some(stash) = plan.folded {
        stash::drop_stash_entry(repo, stash)?;
        refs::delete_ref(repo, &stash::parked_ref(branch), stash, now)?;
    }
    if let Some((id, born)) = &plan.minted {
        let mut meta = branchmeta::read(repo, branch)?;
        meta.change_id = Some(id.clone());
        meta.change_born = Some(*born);
        branchmeta::write(repo, branch, &meta)?;
    }
    let folded = plan.folded.map(|id| id.to_string());
    let everything = |_: &str| true;
    match &plan.what {
        Arrive::None => Ok(ArrivalReport::None),
        Arrive::Resume { open, tree } | Arrive::Replay { open, tree, .. } => {
            let transition =
                worktree::apply_tree_transition(repo, target_tree, *tree, &everything)?;
            let mut files = transition.written;
            files.extend(transition.deleted);
            files.sort();
            files.dedup();
            Ok(ArrivalReport::Restored {
                open: open.to_string(),
                files,
                folded,
            })
        }
        Arrive::Hold { open, paths } => {
            let mut meta = branchmeta::read(repo, branch)?;
            meta.held = Some(Held {
                intent: Intent::Arrive {
                    branch: branch.to_string(),
                    open: open.to_string(),
                },
                at: futures::At::OpenChange,
                paths: paths.clone(),
                time: plan.now,
            });
            meta.change_id = None;
            meta.change_born = None;
            meta.pending_description = None;
            branchmeta::write(repo, branch, &meta)?;
            Ok(ArrivalReport::Held {
                open: open.to_string(),
                paths: paths.clone(),
                folded,
            })
        }
        Arrive::Landed { open } => {
            drop_id(repo, branch)?;
            Ok(ArrivalReport::Landed {
                open: open.to_string(),
                folded,
            })
        }
        Arrive::Invalidate { stash } => {
            refs::delete_ref(repo, &stash::parked_ref(branch), *stash, now)?;
            drop_id(repo, branch)?;
            Ok(ArrivalReport::Invalidated {
                stash: stash.to_string(),
            })
        }
    }
}

/// A parked change that landed or vanished takes its identity with it: the
/// branch arrives with nothing open, so nothing wears the id.
fn drop_id(repo: &gix::Repository, branch: &str) -> Result<()> {
    let mut meta = branchmeta::read(repo, branch)?;
    if meta.change_id.is_some() || meta.change_born.is_some() {
        meta.change_id = None;
        meta.change_born = None;
        branchmeta::write(repo, branch, &meta)?;
    }
    Ok(())
}

/// The parked change waiting on `branch`: its open commit, or a legacy
/// entry's stash commit. A row's question; callers leave out the branch
/// underfoot and branches other worktrees hold, whose open commit is a
/// change that is open, not parked.
pub fn parked(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    if let Some(open) = refs::ref_target(repo, &open::open_ref(branch))? {
        return Ok(Some(open));
    }
    stash::parked_entry(repo, branch)
}

/// How many files an open commit changes against its parent.
pub fn file_count(repo: &gix::Repository, open: gix::ObjectId) -> Result<usize> {
    let (tree, parent) = anatomy(repo, open)?;
    let base = parent_tree(repo, parent)?;
    Ok(crate::changestat::tree_diff_stat(repo, base, tree)?
        .files
        .len())
}

/// The disclosure a rewrite makes of a branch's parked change: that it is
/// there, and whether it still applies on the tip the rewrite leaves. Said,
/// never done. `None` when nothing is parked, and for a branch another
/// worktree holds, whose open commit is that tree's open change.
pub fn disclose(
    repo: &gix::Repository,
    branch: &str,
    new_tip_tree: gix::ObjectId,
) -> Result<Option<crate::model::Parked>> {
    if crate::linked::held_branches(repo)?.contains(branch) {
        return Ok(None);
    }
    if let Some(open) = refs::ref_target(repo, &open::open_ref(branch))? {
        let (tree, parent) = anatomy(repo, open)?;
        let base = parent_tree(repo, parent)?;
        let applies = futures::conflict_paths(repo, base, new_tip_tree, tree)?.is_empty();
        return Ok(Some(crate::model::Parked {
            open: open.to_string(),
            applies,
        }));
    }
    match stash::parked_entry(repo, branch)? {
        Some(id) => {
            let entry = stash::read_stash_commit(repo, id)?;
            let applies =
                futures::conflict_paths(repo, entry.base_tree, new_tip_tree, entry.wip_tree)?
                    .is_empty();
            Ok(Some(crate::model::Parked {
                open: id.to_string(),
                applies,
            }))
        }
        None => Ok(None),
    }
}

/// A verb refusing to leave a dirty tree that has no open commit to stand as
/// its park. A capture writes one whenever it can; the two reasons it could
/// not are said apart.
pub(crate) fn no_park(head: &crate::model::HeadState, branch: &str) -> Error {
    match head {
        crate::model::HeadState::Detached { .. } => {
            Error::msg("detached HEAD: fufu cannot park without a branch")
        }
        _ => Error::msg(format!(
            "the open change on {branch} has no commit to stand as its park: set user.name and \
             user.email so captures can write one, then switch again"
        )),
    }
}
