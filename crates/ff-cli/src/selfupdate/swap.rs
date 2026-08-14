//! Replace the running binary with a new one.
//!
//! Unix: `rename()` is atomic and immune to ETXTBSY (the dirent is replaced,
//! the busy inode is untouched). Windows: a two-step rename via `.old` since
//! the running binary is locked.

/// Resolve the path of the running executable, canonicalizing symlinks.
pub fn resolve_exe() -> ff_core::Result<std::path::PathBuf> {
    let path = std::env::current_exe()
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))?;
    let path = path
        .canonicalize()
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))?;

    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }

    Ok(path)
}

/// Atomically replace the running binary (Unix).
#[cfg(unix)]
pub fn swap(new: &std::path::Path, exe: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(new, exe)
}

/// Replace the running binary on Windows via a `.old` two-step.
#[cfg(windows)]
pub fn swap(new: &std::path::Path, exe: &std::path::Path) -> std::io::Result<()> {
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old); // stale from a previous update; ignore errors
    std::fs::rename(exe, &old)?;
    match std::fs::rename(new, exe) {
        Ok(()) => {
            let _ = std::fs::remove_file(&old); // usually fails while old binary runs — harmless
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::rename(&old, exe); // best-effort restore
            Err(e)
        }
    }
}
