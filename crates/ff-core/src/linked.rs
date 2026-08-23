//! Linked worktrees: who else is standing where.
//!
//! Every worktree of a repository shares one object store and one ref
//! namespace, so fufu's state is shared by construction. Two facts make that
//! safe rather than corrupting, and both are read here.
//!
//! The first is **identity**: [`id`] names the worktree fufu is running in,
//! and the operation log is keyed by it. The name is the worktree's gitdir
//! basename — `main` for the main worktree — which git keeps stable across
//! `git worktree move` and rebuilds on `git worktree repair`. A path would
//! not survive either.
//!
//! The second is **ownership**: git allows a branch in at most one worktree,
//! and [`holders`] is how fufu learns which branches are somebody else's. The
//! answer is read from the worktree HEAD files rather than by spawning `git
//! worktree list` — this sits on the capture path, which every invocation
//! runs, and fufu does not spawn git.
//!
//! `repo.worktrees()` is *linked-only* by gix's own definition, so a linked
//! worktree asking it what is checked out elsewhere cannot see the main
//! worktree at all. The main worktree's HEAD is read separately here, which
//! is the difference between a guard that works in one direction and one that
//! works in both.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The main worktree's chain id. Also the name of a linked worktree created
/// as `git worktree add ../main`, which would share this chain — an
/// unlikely collision that git's own id space allows and fufu inherits.
pub const MAIN_ID: &str = "main";

/// The id of the worktree this repository handle is open on.
///
/// `git_dir() != common_dir()` is exactly gix's own test for "this is a
/// linked worktree", and the basename of a linked gitdir is the id git
/// filed it under in `<common-dir>/worktrees/`.
pub fn id(repo: &gix::Repository) -> String {
    if repo.git_dir() == repo.common_dir() {
        return MAIN_ID.to_string();
    }
    repo.git_dir()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| MAIN_ID.to_string())
}

/// One other worktree, and the branch it is standing on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The worktree's id — the same name its operation chain is keyed by.
    pub id: String,
    /// The branch it holds, short form (`side`, not `refs/heads/side`).
    pub branch: String,
    /// Where it is checked out, for the message git would print.
    pub path: PathBuf,
}

/// Every branch held by a worktree that is *not* this one.
///
/// Bounded by the number of worktrees rather than by history: one directory
/// read plus one small file read apiece, and in the ordinary
/// single-worktree repository the directory read returns `NotFound` and this
/// costs a syscall.
pub fn holders(repo: &gix::Repository) -> Result<Vec<Holder>> {
    let me = id(repo);
    let mut out = Vec::new();

    for proxy in repo.worktrees().map_err(Error::repo)? {
        let wt_id = proxy.id().to_string();
        if wt_id == me {
            continue;
        }
        let Some(branch) = head_branch(&proxy.git_dir().join("HEAD")) else {
            continue; // detached, unreadable, or mid-operation
        };
        out.push(Holder {
            id: wt_id,
            branch,
            path: proxy
                .base()
                .unwrap_or_else(|_| proxy.git_dir().to_path_buf()),
        });
    }

    // The main worktree, which `worktrees()` never lists. A bare repository
    // has none, and `main_worktree_path` is how that is noticed.
    if me != MAIN_ID
        && let Some(branch) = head_branch(&repo.common_dir().join("HEAD"))
        && let Some(path) = main_worktree_path(repo)
    {
        out.push(Holder {
            id: MAIN_ID.to_string(),
            branch,
            path,
        });
    }

    Ok(out)
}

/// The holder of one branch, if another worktree has it.
pub fn holder_of(repo: &gix::Repository, branch: &str) -> Result<Option<Holder>> {
    Ok(holders(repo)?.into_iter().find(|h| h.branch == branch))
}

/// The short branch names held by other worktrees.
pub fn held_branches(repo: &gix::Repository) -> Result<std::collections::HashSet<String>> {
    Ok(holders(repo)?.into_iter().map(|h| h.branch).collect())
}

/// Whether a ref is branch-keyed state belonging to a branch held by another
/// worktree.
///
/// `name` is a full ref name; `held` is the short names from
/// [`held_branches`]. Git allows a branch in at most one worktree, which is
/// what makes branch-keyed state worktree-exclusive: the branch itself and
/// the parked state keyed by it cannot lawfully move under a worktree that
/// does not hold it. Pass the set once per operation rather than resolving
/// it per ref — it is the same for every name in the table.
pub fn owned_elsewhere(name: &str, held: &std::collections::HashSet<String>) -> bool {
    let branch = match name.strip_prefix("refs/heads/") {
        Some(short) => short,
        None => match name.strip_prefix(crate::stash::PARKED_PREFIX) {
            Some(short) => short,
            None => return false,
        },
    };
    held.contains(branch)
}

/// The branch a worktree HEAD file names, short form. `None` when the file
/// is missing, unreadable, or detached — none of which is a branch anyone
/// holds.
fn head_branch(head_path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(head_path).ok()?;
    let target = contents.trim().strip_prefix("ref:")?.trim();
    target
        .strip_prefix("refs/heads/")
        .map(|short| short.to_string())
}

/// Where the main worktree is checked out, or `None` when the repository is
/// bare and there is no main worktree to hold anything.
///
/// The common case is answered without opening a second repository: a
/// gitdir named `.git` sits inside its own worktree. Anything else — a
/// separate git dir, a bare repository with linked worktrees — falls back to
/// asking gix, which is the accurate answer and the rare one.
fn main_worktree_path(repo: &gix::Repository) -> Option<PathBuf> {
    let common = repo.common_dir();
    if common.file_name().is_some_and(|name| name == ".git") {
        return common.parent().map(Path::to_path_buf);
    }
    repo.main_repo()
        .ok()
        .and_then(|main| main.workdir().map(Path::to_path_buf))
}
