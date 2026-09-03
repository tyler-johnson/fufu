//! Where fufu keeps per-user state outside any repository.
//!
//! One resolver per root, injected with the OS name and an environment
//! lookup so it is testable without either. Three lanes read them: the
//! update check's state file and the MCP server's presence marker take the
//! cache root, the extension registry takes the config root, and each pair
//! must agree on its root or a reader looks in a directory a writer never
//! used.
//!
//! The two roots are separate because what they hold is: a cache is state
//! fufu can rebuild and a person may delete, and the config root holds what
//! a person decided. Every platform spells that distinction, so fufu keeps
//! it rather than putting a declaration somewhere a cache sweep would take
//! it.

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

/// The platform config root: `XDG_CONFIG_HOME` or `~/.config` on Linux and
/// the BSDs, `~/Library/Application Support` on macOS, `APPDATA` on
/// Windows. `None` when the environment names nothing, which a reader
/// treats as "nothing declared" and a writer refuses on.
///
/// Windows takes `APPDATA` where the cache root takes `LOCALAPPDATA`,
/// because roaming is what the platform means by config that follows the
/// person.
pub fn config_root_from(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home_join = |suffix: &str| -> Option<PathBuf> {
        let home = env("HOME").filter(|value| !value.is_empty())?;
        Some(PathBuf::from(home).join(suffix))
    };

    match os {
        "macos" => home_join("Library/Application Support"),
        "windows" => env("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        _ => match env("XDG_CONFIG_HOME") {
            Some(val) if !val.is_empty() => Some(PathBuf::from(val)),
            _ => home_join(".config"),
        },
    }
}

/// The config root for the process this is, from its own environment.
pub fn config_root() -> Option<PathBuf> {
    config_root_from(std::env::consts::OS, |name| std::env::var_os(name))
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

    #[test]
    fn config_root_from_linux() {
        let with = |xdg: Option<&str>, home: Option<&str>| {
            let xdg = xdg.map(OsString::from);
            let home = home.map(OsString::from);
            config_root_from("linux", move |key| match key {
                "XDG_CONFIG_HOME" => xdg.clone(),
                "HOME" => home.clone(),
                _ => None,
            })
        };

        assert_eq!(
            with(Some("/custom/config"), Some("/home/user")),
            Some(PathBuf::from("/custom/config")),
        );
        assert_eq!(
            with(None, Some("/home/user")),
            Some(PathBuf::from("/home/user/.config")),
        );
        // Empty XDG falls back the way an unset one does.
        assert_eq!(
            with(Some(""), Some("/home/user")),
            Some(PathBuf::from("/home/user/.config")),
        );
        assert_eq!(with(None, None), None);
        assert_eq!(with(None, Some("")), None);
    }

    #[test]
    fn config_root_from_macos() {
        let mac_env = |key: &str| -> Option<OsString> {
            if key == "HOME" {
                Some(OsString::from("/Users/alice"))
            } else {
                None
            }
        };
        assert_eq!(
            config_root_from("macos", mac_env),
            Some(PathBuf::from("/Users/alice/Library/Application Support")),
        );
    }

    /// The roaming half of the split, and not the one the cache takes.
    #[test]
    fn config_root_from_windows() {
        let win_env = |key: &str| -> Option<OsString> {
            match key {
                "APPDATA" => Some(OsString::from("C:\\Users\\alice\\AppData\\Roaming")),
                "LOCALAPPDATA" => Some(OsString::from("C:\\Users\\alice\\AppData\\Local")),
                _ => None,
            }
        };
        assert_eq!(
            config_root_from("windows", win_env),
            Some(PathBuf::from("C:\\Users\\alice\\AppData\\Roaming")),
        );

        let win_env_empty = |key: &str| -> Option<OsString> {
            if key == "APPDATA" {
                Some(OsString::from(""))
            } else {
                None
            }
        };
        assert_eq!(config_root_from("windows", win_env_empty), None);
    }
}
