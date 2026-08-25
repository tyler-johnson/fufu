//! One spelling for a worktree's path, used everywhere fufu records, prints,
//! or compares one.
//!
//! Resolving symlinks is what git does — `git worktree add` stores the real
//! path and `git worktree remove` resolves its argument the same way, so the
//! two agree however you spell the directory you typed. fufu follows it, for
//! the same reason and for one more: the admin files fufu writes are read by
//! git, so a path git would not recognize is a broken worktree rather than an
//! untidy listing.
//!
//! Two platforms make that harder than calling `canonicalize`. On Windows it
//! returns an extended-length path (`\\?\C:\…`), which git does not parse: it
//! reports the raw string where the worktree's path belongs, and every path
//! fufu prints or records carries a prefix nobody can type. On macOS the
//! temporary directory is reached through a symlink, so the resolved path and
//! the typed one differ by a `/private` nobody typed either — harmless in a
//! listing, fatal to a comparison made with `==`.
//!
//! So: resolve, drop the prefix Windows adds, and compare by identity rather
//! than by spelling.

use std::path::{Component, Path, PathBuf, Prefix};

/// The path fufu records for a worktree: absolute, symlinks resolved, and
/// free of Windows's extended-length prefix.
///
/// A path that cannot be resolved is made absolute instead and returned as it
/// stands. That is not a fallback for the sake of one: a checkout somebody
/// deleted out from under its entry still has a row in the listing and an
/// entry to remove, and it is exactly the case that has no real path left.
pub fn real(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(resolved) => plain(resolved),
        Err(_) => std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }
}

/// A path spelled the way git writes one into the files it reads back:
/// forward slashes, on every platform.
///
/// This is not cosmetic on Windows. git finds a worktree's directory by
/// stripping `/.git` off the end of what `worktrees/<id>/gitdir` holds, and a
/// path spelled with backslashes does not end in `/.git`, so the strip fails
/// silently and git reports the admin file's own path where the worktree's
/// belongs. Everything downstream of `git worktree list` then names something
/// that is not a worktree.
///
/// Only the separator changes: on Unix `MAIN_SEPARATOR` is already `/` and a
/// backslash is an ordinary character in a file name, so this returns the
/// path as it stands rather than corrupting it.
pub fn as_git(path: &Path) -> String {
    let text = path.display().to_string();
    if std::path::MAIN_SEPARATOR == '/' {
        text
    } else {
        text.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

/// Whether two paths name the same worktree.
///
/// The cheap comparison first, since it answers on every platform where the
/// two spellings already agree; [`real`] only when it does not, so the case
/// that costs two directory walks is the one that was going to fail.
pub fn same(a: &Path, b: &Path) -> bool {
    a == b || real(a) == real(b)
}

/// `\\?\C:\dir` as `C:\dir`. Only the verbatim *disk* prefix is dropped —
/// `\\?\UNC\server\share` means something a plain path cannot say, and a
/// device path is not a file path at all, so both are returned untouched.
///
/// On Unix a path has no prefix component, so this returns its argument.
fn plain(path: PathBuf) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let Prefix::VerbatimDisk(letter) = prefix.kind() else {
        return path;
    };
    let mut out = PathBuf::from(format!("{}:\\", letter as char));
    // The root that follows a prefix would send `extend` back to the root of
    // the drive, discarding what was just built.
    out.extend(components.filter(|c| !matches!(c, Component::RootDir)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_no_prefix_is_returned_as_it_stands() {
        let path = PathBuf::from("/tmp/bay");
        assert_eq!(plain(path.clone()), path);
    }

    #[test]
    fn a_git_path_keeps_a_unix_name_with_a_backslash_in_it() {
        // Only meaningful where `/` is the separator, which is where a
        // backslash is a legal character in a name.
        if std::path::MAIN_SEPARATOR == '/' {
            assert_eq!(as_git(Path::new(r"/tmp/a\b/bay")), r"/tmp/a\b/bay");
        }
    }

    #[test]
    fn a_git_path_never_carries_the_native_separator() {
        let rendered = as_git(&real(Path::new(".")));
        assert!(!rendered.contains(std::path::MAIN_SEPARATOR) || std::path::MAIN_SEPARATOR == '/');
    }

    #[test]
    fn a_missing_path_is_still_made_absolute() {
        let real = real(Path::new("no-such-directory-anywhere"));
        assert!(real.is_absolute(), "{real:?}");
    }

    #[test]
    fn the_same_directory_spelled_two_ways_is_the_same_worktree() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(same(here, &here.join("src").join("..")));
        assert!(!same(here, &here.join("src")));
    }
}
