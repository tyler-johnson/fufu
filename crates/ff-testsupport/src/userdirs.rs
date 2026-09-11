//! Where a spawned `ff` keeps per-user state when a test pins its HOME.
//!
//! `ff-cli`'s `userdirs` resolves the cache root per platform, and no one
//! variable redirects it everywhere: linux reads `XDG_CACHE_HOME`, macOS
//! reads only `HOME`, and Windows reads neither and takes `LOCALAPPDATA`.
//! A suite that pins the variable it knows is isolated on the platform it
//! was written on and reads whoever is running it everywhere else.
//!
//! So the layout is spelled here once. [`pin`] sets every variable the
//! resolver consults, and [`config_root`] and [`cache_root`] say where
//! what it pinned will land. The config root is pinned too, so git's own
//! `~/.config/git/config` stays out of a test.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The config root a pinned `ff` resolves under `home`.
pub fn config_root(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".config")
    }
}

/// The cache root a pinned `ff` resolves under `home`. The update check's
/// state file sits below it.
pub fn cache_root(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else {
        home.join(".cache")
    }
}

/// Point both roots at `home` on every platform.
///
/// All five variables are set rather than the two this platform happens to
/// read: the ones it ignores cost nothing, and the call then isolates the
/// same run wherever the suite is built. Windows takes `APPDATA` for config
/// and `LOCALAPPDATA` for cache, the split `userdirs` documents.
pub fn pin<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", config_root(home))
        .env("XDG_CACHE_HOME", cache_root(home))
        .env("APPDATA", config_root(home))
        .env("LOCALAPPDATA", cache_root(home))
}
