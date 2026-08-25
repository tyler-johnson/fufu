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
//!
//! The write half lives in the two submodules: gix has no worktree-creation
//! API, so fufu writes git's on-disk layout itself.

pub mod add;
pub mod path;
pub mod remove;
pub mod survey;

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
            // Through `path::real`, like every other worktree path fufu
            // shows: git writes this entry with forward slashes on Windows,
            // and a message spelling a path differently from the listing is
            // one the reader has to translate.
            path: path::real(
                &proxy
                    .base()
                    .unwrap_or_else(|_| proxy.git_dir().to_path_buf()),
            ),
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
            path: path::real(&path),
        });
    }

    Ok(out)
}

/// Every live worktree's id, this one included — whatever its HEAD is
/// doing.
///
/// Distinct from [`holders`] on purpose. A holder is a worktree standing on
/// a *branch*, which is the question exclusivity asks; this is the question
/// retention asks, and a worktree with a detached HEAD holds no branch while
/// being every bit as alive. Sweeping its chain as an orphan would trim a
/// log out from under a running tree.
pub fn worktree_ids(repo: &gix::Repository) -> Result<Vec<String>> {
    let mut out = vec![MAIN_ID.to_string()];
    for proxy in repo.worktrees().map_err(Error::repo)? {
        out.push(proxy.id().to_string());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The operation chains nobody stands in: every chain whose worktree is
/// gone.
///
/// The live set comes from [`worktree_ids`] rather than [`holders`] on
/// purpose: a worktree with a detached HEAD holds no branch and is every bit
/// as alive, and sweeping its chain would age out a log nobody agreed to
/// lose.
pub fn orphan_chains(repo: &gix::Repository) -> Result<Vec<String>> {
    let live: std::collections::HashSet<String> = worktree_ids(repo)?.into_iter().collect();
    let mut out: Vec<String> = crate::ops::chain_ids(repo)?
        .into_iter()
        .filter(|id| !live.contains(id))
        .collect();
    out.sort();
    Ok(out)
}

/// Capture a linked worktree into its own chain, then tear it down.
///
/// This is the capture-before-courage that makes an undo that deletes a
/// directory acceptable: the chain survives the teardown, so the capture
/// stays addressable, and undoing the undo can put the worktree back on it.
/// Returns the capture's op id when there was one.
pub(crate) fn retire(repo: &gix::Repository, id: &str, now: i64) -> Result<Option<String>> {
    // Absent: there is nothing to retire, and that is not an error — undo
    // must converge rather than refuse over a worktree somebody already
    // removed by hand.
    let Some(proxy) = repo
        .worktrees()
        .map_err(Error::repo)?
        .into_iter()
        .find(|proxy| proxy.id() == id)
    else {
        return Ok(None);
    };

    let capture = match proxy.into_repo() {
        // The checkout is already gone from disk: nothing to capture, and
        // that is not an error. The teardown still runs, because that is the
        // normal way a stale entry gets cleaned up.
        Err(_) => None,
        Ok(wt) => match crate::ops::capture_with(
            &wt,
            &crate::snapshot::Provenance::new("undo", None),
            &crate::snapshot::TakeOptions {
                now: Some(now),
                max_file_size: None,
            },
        )? {
            crate::ops::CaptureOutcome::Created { id, .. } => Some(id.to_string()),
            // A clean bay with a chain already holding its tree needs
            // nothing new; a bay with no operations at all yields `None`,
            // which is honest.
            crate::ops::CaptureOutcome::NoOp { tip, .. } => tip.map(|tip| tip.to_string()),
            crate::ops::CaptureOutcome::Contended => {
                return Err(Error::coded(
                    "worktree/busy",
                    format!("something is running in {id}: its operation log is locked"),
                    vec!["ff worktree list".into()],
                ));
            }
        },
    };

    remove::teardown(repo, id)?;
    Ok(capture)
}

/// Put a linked worktree back where it stood.
///
/// What it stands on, in the order the work is most likely to live in: the
/// capture the effect names; the chain's own tip, where the capture undo
/// took when it retired this worktree lives — the chain outlived the
/// teardown, which is exactly why this works; and then the branch, which a
/// worktree with no history at all comes back at. The id is preserved
/// because the chain is keyed by it: a fresh id would orphan the
/// worktree's own history.
pub(crate) fn revive(
    repo: &gix::Repository,
    id: &str,
    path: &Path,
    branch: &str,
    capture: Option<&str>,
    now: i64,
) -> Result<()> {
    // The destination is free, or the revive does not happen: writing into
    // a directory something else now occupies would merge two lives.
    add::destination_free(path)?;

    // The branch's current tip, what HEAD/ORIG_HEAD name. A gone branch is
    // a warning case for the caller, not a panic — this is the error it
    // turns into one.
    let head =
        crate::refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {branch}"),
                vec!["ff branch list".into()],
            )
        })?;

    let (tree, index_tree) = if let Some(capture) = capture {
        let op = crate::ops::walk::decode(repo, crate::ops::OpId::parse(capture)?.object_id())?;
        let tree = op.tree();
        (tree, op.index_tree()?.unwrap_or(tree))
    } else if let Some(tip) = crate::refs::ref_target(repo, &crate::ops::ops_ref(id))? {
        let op = crate::ops::walk::decode(repo, tip)?;
        let tree = op.tree();
        (tree, op.index_tree()?.unwrap_or(tree))
    } else {
        let tree = repo
            .find_commit(head)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach();
        (tree, tree)
    };

    add::materialize(
        repo,
        add::Layout {
            path,
            id,
            branch,
            tree,
            index_tree,
            head,
        },
        now,
    )?;
    Ok(())
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
