//! The presence marker: how a hook learns that a server is up for the
//! client that is calling it.
//!
//! Keyed by the client process that spawned the server and by the name the
//! server is registered under — `<cache>/fufu/mcp/<client pid>/<name>`. The
//! client process, because `CLAUDE_PID` is what the hook is given, and a
//! session id would not do: the server keeps the id it was launched under
//! while `/clear` hands the hook a new one and the same server keeps
//! running. The name, because a client's file may carry more than one
//! server — fufu's own under `fufu`, and a declared extension's under the
//! extension's name — and one boolean for the pair would answer about the
//! wrong one.
//!
//! Liveness is a lock, not a pid: the server holds an exclusive lock on
//! its marker for its whole life, and the OS releases it on any death,
//! SIGKILL included, so a reader that gets a shared lock knows the file is
//! stale. Stale files are swept by the first reader that finds one, and by
//! every server that starts, directories and all: a hook asks only about
//! its own client, so a marker left by a client that is gone would
//! otherwise sit there forever.
//!
//! A marker claims one thing: the process serving that name for that
//! client is alive, because it is the one holding the lock. Registration
//! on disk says *installed*, and only a held lock says *serving*.
//!
//! fufu's own server holds `fufu`, and that is the only marker fufu
//! writes. A declared extension's server is registered beside fufu's and
//! started by the client as a process of its own, handed no client pid and
//! no obligation here, so nothing fills its name. Nothing under this path
//! is promised to an extension either, which is why a name with no lock
//! behind it reads as a question fufu could not answer rather than as a
//! server that is down — and a reader that has to fail open treats the two
//! the same way.
//!
//! The server side writes, the hook side reads, and both live here so the
//! path and the format have one owner.
//!
//! Every failure on either side is swallowed the way the trigger runtime
//! swallows its own: a marker that could not be held is a refusal that
//! never fires, which is the fail-open direction, and a line on stderr
//! only under `FF_DEBUG`, never on stdout, because the server's stdout is
//! the protocol.

use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<cache_root>/fufu/mcp/`, which holds a directory per client.
pub fn dir() -> Option<PathBuf> {
    dir_under(crate::userdirs::cache_root()?)
}

fn dir_under(root: PathBuf) -> Option<PathBuf> {
    Some(root.join("fufu").join("mcp"))
}

/// The marker for the server registered as `name`, serving `client`.
pub fn path(client: u32, name: &str) -> Option<PathBuf> {
    Some(marker(&dir()?, client, name))
}

fn marker(root: &Path, client: u32, name: &str) -> PathBuf {
    root.join(client.to_string()).join(name)
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

/// The server side: mark fufu's own server as up for its parent, and hold
/// the mark until dropped. `None` when the parent is unknown, when the
/// cache root is, when the file cannot be opened, or when another server
/// for the same parent already holds the lock — and in every case the
/// server serves anyway.
pub fn hold() -> Option<Held> {
    let parent = parent_pid()?;
    let root = dir()?;
    let path = marker(&root, parent, crate::integ::mcp::NAME);
    let held = hold_at(&path, parent).map_err(debug).ok();
    sweep(&root, &path);
    held
}

/// Remove every marker under `root` that no server holds, `keep` excepted,
/// and every client directory left empty by that. Best-effort throughout: a
/// file that cannot be opened, locked, or removed is left for the next
/// sweep, and a held one is a live server's.
pub(crate) fn sweep(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sweep_client(&path, keep);
        } else if path != keep {
            // A marker from before the layout took a directory per client,
            // which nothing reads any more and nothing else would remove.
            serving_at(&path);
        }
    }
}

/// One client's directory swept, and removed when nothing is left in it.
/// The removal fails while any marker remains, which is the guard wanted:
/// a client with a live server keeps its directory.
fn sweep_client(dir: &Path, keep: &Path) {
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
    let _ = std::fs::remove_dir(dir);
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
    // Lock before writing: another server already holding this name for
    // this client must not have its line truncated out from under it.
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

/// The hook side: is the server registered as `name` up for `client`? A
/// name nobody holds a marker for is `false`, which is the fail-open
/// direction — it says nothing was proved, not that nothing is running.
pub fn serving(client: u32, name: &str) -> bool {
    path(client, name).is_some_and(|path| serving_at(&path))
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
        assert!(!serving_at(&marker(dir.path(), 4242, "fufu")));
    }

    #[test]
    fn a_held_marker_is_serving_until_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = marker(dir.path(), 4242, "fufu");
        let held = hold_at(&path, 4242).expect("held");
        assert!(path.is_file(), "the marker was written");
        assert!(serving_at(&path), "a live lock is a live server");
        assert!(path.is_file(), "and a reader leaves it alone");
        // A second server under the same name is refused, not doubled.
        let again = hold_at(&path, 4242);
        assert!(again.is_err(), "the lock is exclusive");
        drop(held);
        assert!(!path.exists(), "dropping the hold removes the marker");
        assert!(!serving_at(&path));
    }

    /// One client, two servers: each name is its own lock, and neither
    /// answers for the other.
    #[test]
    fn a_name_is_held_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let own = marker(dir.path(), 4242, "fufu");
        let theirs = marker(dir.path(), 4242, "tower");
        let held = hold_at(&own, 4242).expect("held");
        assert!(serving_at(&own));
        assert!(
            !serving_at(&theirs),
            "a name nobody holds is not serving, whatever else is"
        );

        let other = hold_at(&theirs, 4242).expect("a second server, a second name");
        assert!(serving_at(&own) && serving_at(&theirs));
        drop(other);
        assert!(serving_at(&own), "one server going does not take the other");
        assert!(!serving_at(&theirs));
        drop(held);
    }

    #[test]
    fn a_sweep_removes_the_stale_and_spares_the_held() {
        let dir = tempfile::tempdir().unwrap();
        let mine = marker(dir.path(), 1, "fufu");
        let held = hold_at(&mine, 1).expect("held");
        // A second name under the sweeper's own client, held by somebody
        // else: the directory is shared and the lock is not.
        let beside = marker(dir.path(), 1, "tower");
        let neighbor = hold_at(&beside, 1).expect("held beside it");
        let live = marker(dir.path(), 2, "fufu");
        let other = hold_at(&live, 2).expect("held by another server");
        let stale = marker(dir.path(), 3, "fufu");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "{\"server\":9}\n").unwrap();
        // The layout before the name was a path segment of its own.
        let old = dir.path().join("4");
        std::fs::write(&old, "{\"server\":9}\n").unwrap();

        sweep(dir.path(), &mine);
        assert!(mine.is_file(), "the sweeper's own marker stays");
        assert!(beside.is_file(), "a marker beside it that is held stays");
        assert!(live.is_file(), "a marker another server holds stays");
        assert!(!stale.exists(), "a marker nobody holds goes");
        assert!(
            !stale.parent().unwrap().exists(),
            "and the client directory it emptied goes with it"
        );
        assert!(!old.exists(), "a marker in the old layout is swept too");
        drop(other);
        drop(neighbor);
        drop(held);
    }

    #[test]
    fn a_stale_marker_is_swept_by_the_first_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = marker(dir.path(), 4242, "fufu");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"server\":1}\n").unwrap();
        assert!(!serving_at(&path), "nobody holds it");
        assert!(!path.exists(), "and it was swept");
    }
}
