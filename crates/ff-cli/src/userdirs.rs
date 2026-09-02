//! Where fufu keeps per-user state outside any repository.
//!
//! One resolver, injected with the OS name and an environment lookup so it
//! is testable without either. Two lanes read it: the update check's state
//! file and the MCP server's presence marker, and they must agree on the
//! root or a reader looks in a directory a writer never used.

use std::ffi::OsString;
use std::path::PathBuf;

/// The platform cache root: `XDG_CACHE_HOME` or `~/.cache` on Linux and
/// the BSDs, `~/Library/Caches` on macOS, `LOCALAPPDATA` on Windows. `None`
/// when the environment names nothing, which every caller treats as
/// "no state", never as an error.
pub fn cache_root_from(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let env_or_home = |ev: &str, home_fallback: &str| -> Option<PathBuf> {
        let val = env(ev)?;
        if val.is_empty() {
            return None;
        }
        let home = PathBuf::from(val);
        Some(home.join(home_fallback))
    };

    match os {
        "macos" => env_or_home("HOME", "Library/Caches"),
        "windows" => {
            let val = env("LOCALAPPDATA")?;
            if val.is_empty() {
                return None;
            }
            Some(PathBuf::from(val))
        }
        _ => match env("XDG_CACHE_HOME") {
            Some(val) if !val.is_empty() => Some(PathBuf::from(val)),
            _ => env_or_home("HOME", ".cache"),
        },
    }
}

/// The cache root for the process this is, from its own environment.
pub fn cache_root() -> Option<PathBuf> {
    cache_root_from(std::env::consts::OS, |name| std::env::var_os(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_from_linux() {
        let linux_env = |key: &str| -> Option<OsString> {
            match key {
                "XDG_CACHE_HOME" => Some(OsString::from("/custom/cache")),
                "HOME" => Some(OsString::from("/home/user")),
                _ => None,
            }
        };
        assert_eq!(
            cache_root_from("linux", linux_env),
            Some(PathBuf::from("/custom/cache")),
        );

        // XDG unset + HOME
        let linux_env2 = |key: &str| -> Option<OsString> {
            if key == "HOME" {
                Some(OsString::from("/home/user"))
            } else {
                None
            }
        };
        assert_eq!(
            cache_root_from("linux", linux_env2),
            Some(PathBuf::from("/home/user/.cache")),
        );

        // XDG empty + HOME → fallback to HOME/.cache
        let linux_env3 = |key: &str| -> Option<OsString> {
            match key {
                "XDG_CACHE_HOME" => Some(OsString::from("")),
                "HOME" => Some(OsString::from("/home/user")),
                _ => None,
            }
        };
        assert_eq!(
            cache_root_from("linux", linux_env3),
            Some(PathBuf::from("/home/user/.cache")),
        );

        // Neither
        let linux_env4 = |_key: &str| -> Option<OsString> { None };
        assert_eq!(cache_root_from("linux", linux_env4), None);
    }

    #[test]
    fn cache_root_from_macos() {
        let mac_env = |key: &str| -> Option<OsString> {
            if key == "HOME" {
                Some(OsString::from("/Users/alice"))
            } else {
                None
            }
        };
        assert_eq!(
            cache_root_from("macos", mac_env),
            Some(PathBuf::from("/Users/alice/Library/Caches")),
        );
    }

    #[test]
    fn cache_root_from_windows() {
        let win_env = |key: &str| -> Option<OsString> {
            if key == "LOCALAPPDATA" {
                Some(OsString::from("C:\\Users\\alice\\AppData\\Local"))
            } else {
                None
            }
        };
        assert_eq!(
            cache_root_from("windows", win_env),
            Some(PathBuf::from("C:\\Users\\alice\\AppData\\Local")),
        );

        // Empty LOCALAPPDATA → None
        let win_env_empty = |key: &str| -> Option<OsString> {
            if key == "LOCALAPPDATA" {
                Some(OsString::from(""))
            } else {
                None
            }
        };
        assert_eq!(cache_root_from("windows", win_env_empty), None);
    }
}
