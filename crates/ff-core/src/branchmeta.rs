//! Per-branch metadata: the pending description (set by `ff new -m` /
//! `ff describe`, consumed by the close) and the fork base (display-only
//! leaf cache, written once when a branch is minted). Plain files under
//! `<common-dir>/fufu/branch/<branch-path>` — the path mirrors the refs
//! layout, so slashes need no encoding and file/directory conflicts are
//! impossible for exactly the names git itself allows. Writes are durable
//! (tmp + sync + rename).

use std::io::Write as _;
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
}

impl BranchMeta {
    pub fn is_empty(&self) -> bool {
        self.pending_description.is_none() && self.forked_from.is_none()
    }
}

fn meta_path(repo: &gix::Repository, branch: &str) -> PathBuf {
    repo.common_dir().join("fufu/branch").join(branch)
}

/// Read a branch's metadata; absent file = empty metadata.
pub fn read(repo: &gix::Repository, branch: &str) -> Result<BranchMeta> {
    match std::fs::read(meta_path(repo, branch)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|err| Error::msg(format!("corrupt branch metadata for {branch}: {err}"))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BranchMeta::default()),
        Err(err) => Err(Error::repo(err)),
    }
}

/// Write a branch's metadata durably; empty metadata deletes the file.
pub fn write(repo: &gix::Repository, branch: &str, meta: &BranchMeta) -> Result<()> {
    let path = meta_path(repo, branch);
    if meta.is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(Error::repo(err)),
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg("branch metadata path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(Error::repo)?;
    let bytes = serde_json::to_vec_pretty(meta).map_err(|err| Error::msg(err.to_string()))?;
    let tmp = parent.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let mut file = std::fs::File::create(&tmp).map_err(Error::repo)?;
    let write = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp, &path)
        });
    if let Err(err) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::repo(err));
    }
    Ok(())
}

/// Move metadata from one branch name to another (rename carry).
pub fn rename(repo: &gix::Repository, old: &str, new: &str) -> Result<()> {
    let meta = read(repo, old)?;
    write(repo, new, &meta)?;
    write(repo, old, &BranchMeta::default())
}
