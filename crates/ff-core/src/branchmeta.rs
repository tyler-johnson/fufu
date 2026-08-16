//! Per-branch metadata: the pending description (set by `ff new -m` /
//! `ff describe`, consumed by the close), the fork base (display-only
//! leaf cache, written once when a branch is minted), and the explicit
//! parent branch a `ff start` forked from. Plain files under
//! `<common-dir>/fufu/branch/<branch-path>` — the path mirrors the refs
//! layout, so slashes need no encoding and file/directory conflicts are
//! impossible for exactly the names git itself allows. Writes go through
//! `crate::jsonfile` (tmp + sync + rename in the destination's directory).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchMeta {
    /// Consumed by the next close on this branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_description: Option<String>,
    /// The commit this branch was forked from, when fufu minted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<String>,
    /// The branch this one was explicitly forked from, when `ff start` was
    /// given a target that named a local branch. Display-only and advisory:
    /// a parent that no longer resolves is simply ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

impl BranchMeta {
    pub fn is_empty(&self) -> bool {
        self.pending_description.is_none() && self.forked_from.is_none() && self.parent.is_none()
    }
}

fn meta_path(repo: &gix::Repository, branch: &str) -> PathBuf {
    repo.common_dir().join("fufu/branch").join(branch)
}

/// Read a branch's metadata; absent file = empty metadata.
pub fn read(repo: &gix::Repository, branch: &str) -> Result<BranchMeta> {
    crate::jsonfile::read(&meta_path(repo, branch))
        .map(|meta| meta.unwrap_or_default())
        .map_err(|err| Error::msg(format!("corrupt branch metadata for {branch}: {err}")))
}

/// Write a branch's metadata durably; empty metadata deletes the file.
pub fn write(repo: &gix::Repository, branch: &str, meta: &BranchMeta) -> Result<()> {
    let path = meta_path(repo, branch);
    if meta.is_empty() {
        crate::jsonfile::remove(&path)
    } else {
        crate::jsonfile::write(&path, meta)
    }
}

/// Move metadata from one branch name to another (rename carry).
pub fn rename(repo: &gix::Repository, old: &str, new: &str) -> Result<()> {
    let meta = read(repo, old)?;
    write(repo, new, &meta)?;
    write(repo, old, &BranchMeta::default())
}
