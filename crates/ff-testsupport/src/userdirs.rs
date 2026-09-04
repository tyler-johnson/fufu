//! Where a spawned `ff` keeps per-user state when a test pins its HOME.
//!
//! `ff-cli`'s `userdirs` resolves the cache and config roots per platform,
//! and no one variable redirects all three: linux reads `XDG_CACHE_HOME`
//! and `XDG_CONFIG_HOME`, macOS reads only `HOME`, and Windows reads
//! neither and takes `LOCALAPPDATA` and `APPDATA`. A suite that pins the
//! pair it knows is isolated on the platform it was written on and reads
//! whoever is running it everywhere else — which is how a corrupt registry
//! written to `$HOME/.config` came back as no registry at all on macOS.
//!
//! So the layout is spelled here once. [`pin`] sets every variable the
//! resolver consults, and [`config_root`], [`cache_root`] and [`registry`]
//! say where what it pinned will land. A test reaching into either root
//! asks for the path rather than joining one, so the directory it writes
//! and the directory the binary reads cannot drift apart.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The config root a pinned `ff` resolves under `home`. The extension
/// registry sits below it.
pub fn config_root(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".config")
    }
}

/// The cache root a pinned `ff` resolves under `home`. The update check's
/// state file and the MCP server's presence markers sit below it.
pub fn cache_root(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else {
        home.join(".cache")
    }
}

/// The extension registry file under that config root, which is the one
/// path a test should ever write a declaration to.
pub fn registry(home: &Path) -> PathBuf {
    config_root(home).join("fufu").join("extensions.json")
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
