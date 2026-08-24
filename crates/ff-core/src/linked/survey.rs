//! The survey: every live worktree, and every chain whose worktree is gone.
//!
//! [super] reads who is standing where one question at a time; this answers
//! the whole question at once, and adds the second kind of row — the chains
//! nobody stands in — because the chain outlives the worktree on purpose,
//! and a deleted bay's work is findable through exactly its tip.

use crate::error::{Error, Result};
use crate::model::{OrphanRow, Survey, WorktreeRow};
use crate::ops::OpId;

/// Every live worktree, main first, and every orphan chain, in the order
/// [`super::orphan_chains`] gives them.
pub fn survey(repo: &gix::Repository) -> Result<Survey> {
    let me = super::id(repo);
    let mut worktrees = Vec::new();

    // The main worktree first, where a reader looks for it. `repo.worktrees()`
    // is linked-only by gix's own definition, so this row is built separately
    // rather than listed.
    let chain = crate::ops::ops_ref(super::MAIN_ID);
    worktrees.push(WorktreeRow {
        id: super::MAIN_ID.to_string(),
        // Absolute, like every other row: gix reports a workdir relative to
        // the cwd it was discovered from, so the main worktree would
        // otherwise list as `.` while the linked ones list as full paths.
        path: repo
            .main_repo()
            .ok()
            .and_then(|main| main.workdir().map(absolute)),
        branch: super::head_branch(&repo.common_dir().join("HEAD")),
        tip: live_tip(repo, &chain)?,
        chain,
        current: me == super::MAIN_ID,
    });

    for proxy in repo.worktrees().map_err(Error::repo)? {
        let id = proxy.id().to_string();
        let chain = crate::ops::ops_ref(&id);
        let current = me == id;
        worktrees.push(WorktreeRow {
            id,
            path: proxy.base().ok().map(|base| absolute(&base)),
            branch: super::head_branch(&proxy.git_dir().join("HEAD")),
            tip: live_tip(repo, &chain)?,
            chain,
            current,
        });
    }

    let mut orphans = Vec::new();
    for id in super::orphan_chains(repo)? {
        let chain = crate::ops::ops_ref(&id);
        // A chain id comes from `chain_ids`, so the ref exists; `None` means
        // it vanished in the meantime, and a row of three `None`s would say
        // nothing, so the chain is skipped entirely.
        let Some(sha) = crate::refs::ref_target(repo, &chain)? else {
            continue;
        };
        // A decode that fails must not fail the survey: an orphan chain is by
        // definition one nobody is maintaining, and it can have been trimmed
        // to a tip whose object is gone, or written by a newer binary. A
        // listing that errors because one dead bay is unreadable is worse
        // than one that shows the row with what it could read, so carry on
        // with the id alone and the rest unknown.
        match crate::ops::walk::decode(repo, sha) {
            Ok(op) => orphans.push(OrphanRow {
                id,
                chain,
                tip: Some(op.id().to_string()),
                branch: op.branch().map(str::to_string),
                time: Some(op.time()),
            }),
            Err(_) => orphans.push(OrphanRow {
                id,
                chain,
                // Still the letters spelling: `tip` is what
                // `ff restore --at-op` takes, and a hex sha would not resolve
                // there. An unreadable record costs the branch and the time,
                // not the address.
                tip: Some(OpId::new(sha).to_string()),
                branch: None,
                time: None,
            }),
        }
    }

    Ok(Survey { worktrees, orphans })
}

/// The chain's newest operation, in the letters spelling. The id is all a
/// live row carries — decoding per worktree would buy nothing — so this
/// reads the ref and stops there.
fn live_tip(repo: &gix::Repository, chain: &str) -> Result<Option<String>> {
    Ok(crate::refs::ref_target(repo, chain)?.map(|sha| OpId::new(sha).to_string()))
}

/// A path made absolute without requiring it to exist. `canonicalize` would
/// refuse a checkout that has been deleted out from under its entry, which is
/// exactly the case this listing has to survive.
fn absolute(path: &std::path::Path) -> std::path::PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
