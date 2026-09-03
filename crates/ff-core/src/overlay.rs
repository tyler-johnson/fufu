//! The world as a run has planned it, before anything is written.
//!
//! A verb that moves several branches in one operation plans each after the
//! last, and every plan after the first reads a repository that has not
//! changed yet: the refs still stand where the run found them, no hold is on
//! any branch's metadata, and the working tree still holds the open change
//! against the tip HEAD was on. The overlay carries what the run has decided
//! so far, and the planners read it before they read the repository, so a
//! child planned after its parent sits on the parent's planned tip and a
//! branch the run already holds is not planned twice. `ff sync` is the
//! caller: one operation for everything it moved, written ahead of the first
//! ref move the way `ff restack` writes its own.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::held::{self, Held};
use crate::refs;
use crate::snapshot::tree as snaptree;

/// Where the working tree goes, when HEAD's branch is one the run moved.
pub(crate) struct PlannedHead {
    /// The open change as the run found it: the exact worktree tree, the
    /// same on every move so the one write at the end starts from what the
    /// files hold.
    pub open: gix::ObjectId,
    /// The tip HEAD's branch will stand on.
    pub tip: gix::ObjectId,
    /// The tree the worktree will hold: the open change carried onto `tip`.
    pub worktree: gix::ObjectId,
}

/// What a run has planned and not written: tips by full ref name, holds by
/// branch, and the working tree's move.
#[derive(Default)]
pub(crate) struct Overlay {
    tips: BTreeMap<String, gix::ObjectId>,
    holds: BTreeMap<String, Held>,
    pub head: Option<PlannedHead>,
}

impl Overlay {
    /// The planned tip of a full ref, when the run moved it.
    pub fn tip(&self, full: &str) -> Option<gix::ObjectId> {
        self.tips.get(full).copied()
    }

    /// A ref's tip as the run sees it: planned first, then the repository.
    pub fn ref_target(&self, repo: &gix::Repository, full: &str) -> Result<Option<gix::ObjectId>> {
        match self.tip(full) {
            Some(id) => Ok(Some(id)),
            None => refs::ref_target(repo, full),
        }
    }

    /// A local branch's tip as the run sees it.
    pub fn branch_tip(
        &self,
        repo: &gix::Repository,
        branch: &str,
    ) -> Result<Option<gix::ObjectId>> {
        self.ref_target(repo, &format!("refs/heads/{branch}"))
    }

    pub fn set_tip(&mut self, full: &str, id: gix::ObjectId) {
        self.tips.insert(full.to_string(), id);
    }

    /// The hold standing on a branch as the run sees it: planned first,
    /// then the branch's metadata.
    pub fn held(&self, repo: &gix::Repository, branch: &str) -> Result<Option<Held>> {
        match self.holds.get(branch) {
            Some(h) => Ok(Some(h.clone())),
            None => held::of(repo, branch),
        }
    }

    /// A hold the run planned on `branch`.
    pub fn has_hold(&self, branch: &str) -> bool {
        self.holds.contains_key(branch)
    }

    pub fn hold(&mut self, branch: &str, held: Held) {
        self.holds.insert(branch.to_string(), held);
    }

    /// The open change of HEAD's branch, whose tip's tree is `tip_tree`: the
    /// worktree the run has planned when it already moved that branch, and
    /// otherwise the files as they stand, assembled onto the tip's tree with
    /// nothing size-capped out.
    pub fn open_tree(
        &self,
        repo: &gix::Repository,
        tip_tree: gix::ObjectId,
    ) -> Result<gix::ObjectId> {
        if let Some(head) = &self.head {
            return Ok(head.worktree);
        }
        let scan = snaptree::scan(repo)?;
        if scan.is_empty() {
            return Ok(tip_tree);
        }
        Ok(snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?.0)
    }

    /// HEAD's branch moved again: `open` is what the move started from,
    /// which is the run's planned worktree after the first move, so the
    /// original open change is kept and only the destination advances.
    pub fn move_head(&mut self, open: gix::ObjectId, tip: gix::ObjectId, worktree: gix::ObjectId) {
        match self.head.as_mut() {
            Some(head) => {
                head.tip = tip;
                head.worktree = worktree;
            }
            None => {
                self.head = Some(PlannedHead {
                    open,
                    tip,
                    worktree,
                });
            }
        }
    }
}
