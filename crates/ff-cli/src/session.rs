//! Session plumbing: resolve the current session, normalize names, and
//! manage the marker file.
//!
//! The marker (`<common-dir>/fufu/session`) is **not repository state**.
//! It is not journaled, `ff undo` does not restore it, and `ff trim` does
//! not touch it. It is a per-repo working note that tells the capture
//! layer which session name to stamp onto snapshot commit trailers.

use std::io::Write as _;
use std::path::PathBuf;

use ff_core::error::{Error, Result};
use ff_core::gix;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMarker {
    pub name: String,
    pub started: u64,
}

/// Normalize a raw session name per the rules: lowercase, replace every
/// non-alphanumeric character (except `-`) with `-`, collapse runs of `-`,
/// trim leading/trailing `-`, truncate to 32 characters. Returns `None`
/// when nothing survives normalization.
pub fn normalize(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    // Collapse runs of `-` (the loop above prevents adjacent `-` but a
    // trailing `-` from the replacement + leading `-` from the next char
    // can still happen if the input has consecutive non-allowed chars).
    let out: String = out
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-");
    let out: String = out.chars().take(32).collect();
    if out.is_empty() { None } else { Some(out) }
}

/// The session a snapshot taken right now belongs to, or `None`.
/// `FF_SESSION` in the environment wins over the on-disk marker.
pub fn current(repo: &gix::Repository) -> Option<SessionMarker> {
    // Environment variable wins: scripts and agent hooks set this because
    // it is process-scoped and needs no cleanup.
    if let Some(env_val) = std::env::var_os("FF_SESSION")
        && let Some(name) = normalize(&env_val.to_string_lossy())
    {
        return Some(SessionMarker {
            name,
            started: now_secs(),
        });
    }
    // Fall back to the on-disk marker.
    read_marker(repo)
}

/// Generate a session name from the petname wordlists (same source as
/// anonymous branches, but without the `ff/` prefix).
pub fn generate_name() -> String {
    ff_core::petname::generate_name()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn marker_path(repo: &gix::Repository) -> PathBuf {
    repo.common_dir().join("fufu/session")
}

/// Read the session marker file; absent file means no session is open.
pub fn read_marker(repo: &gix::Repository) -> Option<SessionMarker> {
    let path = marker_path(repo);
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Read the on-disk marker only (ignoring the environment). Returns just
/// the name so the command layer can report which session was replaced.
pub fn read_current(repo: &gix::Repository) -> Option<String> {
    read_marker(repo).map(|m| m.name)
}

/// Write the session marker durably (tmp + sync + rename).
pub fn write_marker(repo: &gix::Repository, name: &str) -> Result<SessionMarker> {
    let path = marker_path(repo);
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg("session marker path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(Error::repo)?;

    let marker = SessionMarker {
        name: name.to_string(),
        started: now_secs(),
    };
    let bytes = serde_json::to_vec(&marker).map_err(|err| Error::msg(err.to_string()))?;
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
    Ok(marker)
}

/// Remove the session marker. A no-op when the file is already absent.
pub fn remove_marker(repo: &gix::Repository) -> Result<()> {
    let path = marker_path(repo);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::repo(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_basic() {
        assert_eq!(
            normalize("Refactor Parser!"),
            Some("refactor-parser".into())
        );
    }

    #[test]
    fn normalize_truncates_to_32() {
        let input = "a".repeat(60);
        let result = normalize(&input).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn normalize_all_invalid_is_none() {
        assert_eq!(normalize("---"), None);
    }

    #[test]
    fn normalize_special_chars_become_dashes() {
        assert_eq!(normalize("hello world!"), Some("hello-world".into()));
        assert_eq!(normalize("a.b/c\\d"), Some("a-b-c-d".into()));
    }
}
