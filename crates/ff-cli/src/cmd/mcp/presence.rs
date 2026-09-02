//! The presence marker: how a hook learns that a fufu server is up for the
//! client that is calling it.
//!
//! Keyed by the client process that spawned the server, because `CLAUDE_PID`
//! is what the hook is given, and a session id would not do — the server
//! keeps the id it was launched under while `/clear` hands the hook a new
//! one and the same server keeps running. Liveness is a lock, not a pid:
//! the server holds an exclusive lock on its marker for its whole life, and
//! the OS releases it on any death, SIGKILL included, so a reader that gets
//! a shared lock knows the file is stale. Stale files are swept by the
//! first reader that finds one, and by every server that starts: a hook
//! asks only about its own client, so a marker left by a client that is
//! gone would otherwise sit there forever.
//!
//! The server side writes, the hook side reads, and both live here so the
//! path and the format have one owner. Registration on disk says
//! *installed*; only this says *serving*.
//!
//! Every failure on either side is swallowed the way the trigger runtime
//! swallows its own: a marker that could not be held is a refusal that
//! never fires, which is the fail-open direction, and a line on stderr
//! only under `FF_DEBUG`, never on stdout, because the server's stdout is
//! the protocol.

use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<cache_root>/fufu/mcp/`, where every marker lives.
pub fn dir() -> Option<PathBuf> {
    dir_under(crate::userdirs::cache_root()?)
}

fn dir_under(root: PathBuf) -> Option<PathBuf> {
    Some(root.join("fufu").join("mcp"))
}

/// The marker for a server whose parent is `parent`.
pub fn path(parent: u32) -> Option<PathBuf> {
    Some(dir()?.join(parent.to_string()))
}

/// The pid of the process that spawned this one: the client, when this
/// process is `ff mcp`.
#[cfg(unix)]
pub fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

/// The same, walked out of a process snapshot, because Windows has no
/// `getppid`. `None` on any failure.
#[cfg(windows)]
pub fn parent_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    // SAFETY: a process snapshot is a handle this function owns and closes
    // on every path past this line; the entry is a plain C struct whose
    // `dwSize` the API requires set before the first read, and every read
    // of it happens only after the call that filled it returned nonzero.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let me = GetCurrentProcessId();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == me {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

/// A held marker. The lock lives as long as this does, and dropping it
/// releases the lock and removes the file, in that order.
pub struct Held {
    file: Option<File>,
    path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        // The lock goes with the file handle; only then is the path free
        // to remove without a reader briefly seeing a locked, unlinked
        // inode.
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The server side: mark this server as up for its parent, and hold the
/// mark until dropped. `None` when the parent is unknown, when the cache
/// root is, when the file cannot be opened, or when another server for the
/// same parent already holds the lock — and in every case the server
/// serves anyway.
pub fn hold() -> Option<Held> {
    let parent = parent_pid()?;
    let path = path(parent)?;
    let held = hold_at(&path, parent).map_err(debug).ok();
    if let Some(dir) = path.parent() {
        sweep(dir, &path);
    }
    held
}

/// Remove every marker under `dir` that no server holds, `keep` excepted.
/// Best-effort throughout: a file that cannot be opened, locked, or removed
/// is left for the next sweep, and a held one is a live server's.
pub(crate) fn sweep(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || !path.is_file() {
            continue;
        }
        serving_at(&path);
    }
}

/// [`hold`] against one path, for the tests that drive it in-process.
pub(crate) fn hold_at(path: &Path, parent: u32) -> std::io::Result<Held> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // Lock before writing: another server for this parent already holding
    // the file must not have its line truncated out from under it.
    file.try_lock().map_err(|err| match err {
        TryLockError::Error(err) => err,
        TryLockError::WouldBlock => std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!("another server already marks {}", path.display()),
        ),
    })?;
    // Diagnostic only: nothing reads the fields, and the lock is the
    // whole of the liveness signal.
    file.set_len(0)?;
    let line = serde_json::json!({
        "server": std::process::id(),
        "parent": parent,
        "version": env!("CARGO_PKG_VERSION"),
    });
    writeln!(file, "{line}")?;
    Ok(Held {
        file: Some(file),
        path: path.to_path_buf(),
    })
}

/// The hook side: is a server up for `parent`?
pub fn serving(parent: u32) -> bool {
    path(parent).is_some_and(|path| serving_at(&path))
}

/// [`serving`] against one path. Missing is `false`. A shared lock refused
/// means a live server holds the exclusive one, so `true`. A shared lock
/// granted means the server that wrote it is gone: the file is swept and
/// the answer is `false`. Any other failure is `false`, the fail-open
/// direction.
pub(crate) fn serving_at(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    match file.try_lock_shared() {
        Err(TryLockError::WouldBlock) => true,
        Ok(()) => {
            let _ = file.unlock();
            drop(file);
            let _ = std::fs::remove_file(path);
            false
        }
        Err(TryLockError::Error(err)) => {
            debug(err);
            false
        }
    }
}

fn debug(err: std::io::Error) -> std::io::Error {
    if std::env::var_os("FF_DEBUG").is_some() {
        eprintln!("ff[debug]: mcp: presence: {err}");
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dir_hangs_off_the_cache_root() {
        let root = crate::userdirs::cache_root_from("linux", |key| match key {
            "XDG_CACHE_HOME" => Some("/custom/cache".into()),
            _ => None,
        })
        .expect("a root");
        assert_eq!(
            dir_under(root),
            Some(PathBuf::from("/custom/cache/fufu/mcp"))
        );
    }

    #[test]
    fn a_missing_marker_is_not_serving() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!serving_at(&dir.path().join("4242")));
    }

    #[test]
    fn a_held_marker_is_serving_until_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp").join("4242");
        let held = hold_at(&path, 4242).expect("held");
        assert!(path.is_file(), "the marker was written");
        assert!(serving_at(&path), "a live lock is a live server");
        assert!(path.is_file(), "and a reader leaves it alone");
        // A second server for the same parent is refused, not doubled.
        let again = hold_at(&path, 4242);
        assert!(again.is_err(), "the lock is exclusive");
        drop(held);
        assert!(!path.exists(), "dropping the hold removes the marker");
        assert!(!serving_at(&path));
    }

    #[test]
    fn a_sweep_removes_the_stale_and_spares_the_held() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("1");
        let held = hold_at(&mine, 1).expect("held");
        let live = dir.path().join("2");
        let other = hold_at(&live, 2).expect("held by another server");
        let stale = dir.path().join("3");
        std::fs::write(&stale, "{\"server\":9}\n").unwrap();
        let noise = dir.path().join("notes");
        std::fs::create_dir(&noise).unwrap();

        sweep(dir.path(), &mine);
        assert!(mine.is_file(), "the sweeper's own marker stays");
        assert!(live.is_file(), "a marker another server holds stays");
        assert!(!stale.exists(), "a marker nobody holds goes");
        assert!(noise.is_dir(), "only files are read");
        drop(other);
        drop(held);
    }

    #[test]
    fn a_stale_marker_is_swept_by_the_first_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("4242");
        std::fs::write(&path, "{\"server\":1}\n").unwrap();
        assert!(!serving_at(&path), "nobody holds it");
        assert!(!path.exists(), "and it was swept");
    }
}
