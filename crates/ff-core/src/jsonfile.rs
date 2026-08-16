//! Durable small-JSON file IO, shared by the per-branch metadata and the
//! futures cache. The write is a temp file in the destination's own
//! directory, `sync_all`ed, then renamed over the destination — rename is
//! atomic only within one filesystem, hence the same-directory temp — so a
//! torn write is never observable as a valid file.

use std::io::Write as _;
use std::path::Path;

use crate::error::{Error, Result};

/// Read and deserialize a JSON file; an absent file is `Ok(None)`.
pub(crate) fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|err| Error::msg(format!("corrupt {}: {err}", path.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::repo(err)),
    }
}

/// Serialize and durably write a JSON file, creating the parent directory.
pub(crate) fn write<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg(format!("{} has no parent directory", path.display())))?;
    std::fs::create_dir_all(parent).map_err(Error::repo)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| Error::msg(err.to_string()))?;
    let tmp = parent.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    // Hold the write handle across `sync_all` — on Windows a read-only
    // handle cannot be flushed.
    let mut file = std::fs::File::create(&tmp).map_err(Error::repo)?;
    let write = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp, path)
        });
    if let Err(err) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::repo(err));
    }
    Ok(())
}

/// Delete a file; an absent one is not an error.
pub(crate) fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::repo(err)),
    }
}
