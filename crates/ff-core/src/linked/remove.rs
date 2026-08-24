//! Removing a linked worktree: fufu tears down the checkout and the
//! administrative directory, and touches nothing else.

use std::path::PathBuf;

use crate::error::{Error, Result};

/// A linked worktree fufu removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub id: String,
    /// Where the checkout stood, or `None` when the entry no longer named
    /// one — a worktree somebody deleted by hand leaves an administrative
    /// entry behind, and tearing that down is the normal way it gets cleaned
    /// up.
    pub path: Option<PathBuf>,
}

/// Delete a linked worktree's checkout and its administrative directory, and
/// nothing else.
///
/// This captures nothing and refuses nothing about the tree's contents; it
/// is the raw teardown, and the capture-before-courage belongs to the verb in
/// a later brief, so nobody wires it to a verb without one.
///
/// The chain at `refs/fufu/wt/<id>/ops` survives a removal on purpose — that
/// is the previous landing's guarantee, and the reason a deleted bay's work
/// stays reachable.
pub fn teardown(repo: &gix::Repository, id: &str) -> Result<Removed> {
    if id == crate::linked::MAIN_ID {
        return Err(Error::coded(
            "worktree/is-main",
            "the main worktree cannot be removed",
            vec!["git worktree list".into()],
        ));
    }

    let proxy = repo
        .worktrees()
        .map_err(Error::repo)?
        .into_iter()
        .find(|proxy| proxy.id() == id)
        .ok_or_else(|| {
            Error::coded(
                "worktree/not-found",
                format!("no linked worktree named {id}"),
                vec!["git worktree list".into()],
            )
        })?;

    // The checkout path, if the entry still names one. An entry whose
    // checkout is already gone is still torn down — that is the normal way a
    // stale entry gets cleaned up.
    let path = proxy.base().ok();

    if let Some(checkout) = &path {
        remove_dir_all_lenient(checkout)?;
    }
    remove_dir_all_lenient(&repo.common_dir().join("worktrees").join(id))?;

    Ok(Removed {
        id: id.to_string(),
        path,
    })
}

/// `remove_dir_all`, where `NotFound` is success, not an error.
fn remove_dir_all_lenient(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::repo(err)),
    }
}
