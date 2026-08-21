//! The passive lane's state file; the spawn/pending machinery; the cadence
//! grammar has been extracted to [`crate::cadence`].

use serde::{Deserialize, Serialize};
use std::io::IsTerminal;

/// update.json — all timestamps are unix seconds. `interval_secs` caches the
/// parsed `fufu.updateCheck` so the hot path is one file read, no config load:
/// 0 = unset/default, -1 = disabled, else seconds.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    pub checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<String>,
    pub auto_tried_at: i64,
    pub interval_secs: i64,
}

/// Resolve the platform cache root using a pure env-lookup closure.
pub fn cache_root_from(
    os: &str,
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let env_or_home = |ev: &str, home_fallback: &str| -> Option<std::path::PathBuf> {
        let val = env(ev)?;
        if val.is_empty() {
            return None;
        }
        let home = std::path::PathBuf::from(val);
        Some(home.join(home_fallback))
    };

    match os {
        "macos" => env_or_home("HOME", "Library/Caches"),
        "windows" => {
            let val = env("LOCALAPPDATA")?;
            if val.is_empty() {
                return None;
            }
            Some(std::path::PathBuf::from(val))
        }
        _ => match env("XDG_CACHE_HOME") {
            Some(val) if !val.is_empty() => Some(std::path::PathBuf::from(val)),
            _ => env_or_home("HOME", ".cache"),
        },
    }
}

/// Path to the passive-lane state file (`<cache_root>/fufu/update.json`).
pub fn state_path() -> Option<std::path::PathBuf> {
    let root = cache_root_from(std::env::consts::OS, |n| std::env::var_os(n))?;
    Some(root.join("fufu").join("update.json"))
}

/// Load the passive-lane state from `path`.
///
/// Any error (missing, unreadable, corrupt) returns [`UpdateState::default`].
pub fn load_state(path: &std::path::Path) -> UpdateState {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Save the passive-lane state to `path` using atomic temp-file + rename.
pub fn save_state(path: &std::path::Path, state: &UpdateState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name("update.json.ff-tmp");
    let body = serde_json::to_string(state)?;
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Auto-install probes are hard-coded daily, independent of the check cadence.
const AUTO_RETRY_SECS: i64 = 86_400;

/// Current unix timestamp in seconds.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Passive-lane gate: official build and not in CI.
fn gates_open() -> bool {
    crate::selfupdate::OFFICIAL && std::env::var_os("CI").is_none()
}

/// Spawn a detached process (all stdio nulled, cwd inherited).
fn spawn_detached(exe: &std::path::Path, args: &[&str]) {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (winbase.h) — hardcoded; no winapi dep for one flag.
        cmd.creation_flags(0x0800_0000);
    }
    // Drop the Child: the parent is short-lived, init reaps the orphan.
    let _ = cmd.spawn();
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CheckStatus {
    Unofficial,
    NoCheckYet,
    Available(String), // the newer release tag, e.g. "v0.2.0"
    UpToDate,
}

/// Pure core: no IO, no compile-time gates, fully testable.
pub(crate) fn check_status_from(
    official: bool,
    state: &UpdateState,
    current: Option<crate::selfupdate::Version>,
) -> CheckStatus {
    if !official {
        return CheckStatus::Unofficial;
    }
    let cur = match current {
        Some(v) => v,
        None => return CheckStatus::NoCheckYet,
    };
    let (latest_ver, latest_tag) = match &state.latest {
        Some(tag) => match crate::selfupdate::parse_tag(tag) {
            Some(v) => (v, tag.clone()),
            None => return CheckStatus::NoCheckYet,
        },
        None => return CheckStatus::NoCheckYet,
    };
    if latest_ver > cur {
        return CheckStatus::Available(latest_tag);
    }
    CheckStatus::UpToDate
}

/// The IO wrapper doctor calls: OFFICIAL + the state file + parse_semver.
pub(crate) fn check_status(current_version: &str) -> CheckStatus {
    let state = match state_path() {
        Some(p) => load_state(&p),
        None => UpdateState::default(),
    };
    check_status_from(
        crate::selfupdate::OFFICIAL,
        &state,
        crate::selfupdate::parse_semver(current_version),
    )
}

/// Result of the passive decision core — which actions are due.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Due {
    pub notice: bool, // a not-yet-announced newer release exists
    pub auto: bool,   // an auto-install probe is allowed now
    pub latest: String,
}

/// The pending() decision, minus all IO. None = fast path, nothing due.
pub(crate) fn compute_due(
    state: &UpdateState,
    current: crate::selfupdate::Version,
    now: i64,
    brew: bool,
    tty: bool,
) -> Option<Due> {
    if !tty {
        return None;
    }
    let latest = state.latest.as_ref()?;
    let latest_ver = crate::selfupdate::parse_tag(latest)?;
    if latest_ver <= current {
        return None;
    }
    let notice = state.notified.as_deref() != state.latest.as_deref();
    let auto = !brew && now - state.auto_tried_at >= AUTO_RETRY_SECS;
    if !notice && !auto {
        return None;
    }
    Some(Due {
        notice,
        auto,
        latest: latest.clone(),
    })
}

/// The pending() notice, minus all IO. None = nothing to say to the caller.
pub(crate) fn notice_for(
    due: &Due,
    want_notice: bool,
    current_version: &str,
    brew: bool,
) -> Option<String> {
    if !due.notice || !want_notice {
        return None;
    }
    let suffix = if brew {
        " — update with: brew upgrade fufu"
    } else {
        " — update with: ff update"
    };
    Some(format!(
        "ff: {} is available (running v{}){}",
        due.latest, current_version, suffix
    ))
}

/// Background cache-refresh spawn. Never errors, returns ().
pub fn maybe_spawn_check(repo: &ff_core::gix::Repository) {
    if !gates_open() {
        return;
    }
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    let now = now_secs();

    // Staleness gate on the CACHED interval (hot path — one file read, zero config loads).
    let stale_after = crate::cadence::stale_after(state.interval_secs);
    if now - state.checked_at < stale_after {
        return;
    }

    // Only now read live config (jog parity — scope-agnostic, repo wins).
    let encoded =
        crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.updateCheck");
    state.interval_secs = encoded;

    // Disabled: stamp to prevent daily config re-reads from becoming frequent file writes.
    if encoded == -1 {
        state.checked_at = now;
        let _ = save_state(&path, &state);
        return;
    }

    // Still fresh under the LIVE cadence — persist the encoding and return.
    if let Some(interval) = crate::cadence::effective(encoded)
        && now - state.checked_at < interval
    {
        let _ = save_state(&path, &state);
        return;
    }

    // Stale — spawn a detached check. checked_at is NOT stamped here;
    // the spawned child stamps it first thing, which stops respawn storms when offline.
    let _ = save_state(&path, &state);
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    spawn_detached(&exe, &["update", "--check"]);
}

/// Check whether a release notice or auto-install is pending.
/// Returns a notice string if something should be printed.
///
/// The auto-install half always runs; `want_notice` decides only whether a
/// caller is handed a string to print.
pub fn pending(
    repo: &ff_core::gix::Repository,
    current_version: &str,
    want_notice: bool,
) -> Option<String> {
    if !gates_open() {
        return None;
    }
    let path = state_path()?;
    let state = load_state(&path);
    let tty = std::io::stderr().is_terminal();

    let current = crate::selfupdate::parse_semver(current_version)?;
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let brew = exe.as_deref().is_some_and(|e| {
        crate::selfupdate::classify_install(e, true) == crate::selfupdate::InstallKind::Homebrew
    });

    let due = compute_due(&state, current, now_secs(), brew, tty)?;

    // Something is due — NOW check live config.
    if crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.updateCheck") == -1 {
        return None;
    }

    // Auto-install path.
    if due.auto
        && let Some(exe) = exe
    {
        let mut state = state;
        state.auto_tried_at = now_secs();
        let _ = save_state(&path, &state);

        let auto_update = repo
            .config_snapshot()
            .boolean("fufu.autoUpdate")
            .unwrap_or(true);

        if auto_update {
            spawn_detached(&exe, &["update"]);
            return None;
        }
        // autoUpdate false — fall through to the notice.
    }

    // Notice path.
    notice_for(&due, want_notice, current_version, brew)
}

/// Mark the current latest as notified — a release announces at most once, ever.
pub fn mark_notified() {
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    state.notified = state.latest.clone();
    let _ = save_state(&path, &state);
}

/// Write-through cache sync: keep the cached interval honest when config changes.
/// NOT gated on gates_open — config writes should keep the cache honest everywhere.
pub fn sync_interval(encoded: i64) {
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    state.interval_secs = encoded;
    let _ = save_state(&path, &state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

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
            Some(std::path::PathBuf::from("/custom/cache")),
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
            Some(std::path::PathBuf::from("/home/user/.cache")),
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
            Some(std::path::PathBuf::from("/home/user/.cache")),
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
            Some(std::path::PathBuf::from("/Users/alice/Library/Caches")),
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
            Some(std::path::PathBuf::from("C:\\Users\\alice\\AppData\\Local")),
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
    fn state_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.json");

        // Default round-trips
        let state = UpdateState::default();
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);

        // Fully populated round-trips
        let state = UpdateState {
            checked_at: 1_700_000_000,
            latest: Some("v0.2.0".into()),
            notified: Some("v0.2.0".into()),
            auto_tried_at: 1_700_000_100,
            interval_secs: 86_400,
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);
    }

    #[test]
    fn state_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        assert_eq!(load_state(&path), UpdateState::default());

        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "not json at all {{{").unwrap();
        assert_eq!(load_state(&bad), UpdateState::default());
    }

    // ------------------------------------------------------------------
    // compute_due matrix — pure decision logic, no IO
    // ------------------------------------------------------------------

    fn state_builder(
        latest: Option<&str>,
        notified: Option<&str>,
        auto_tried_at: i64,
    ) -> UpdateState {
        UpdateState {
            latest: latest.map(str::to_string),
            notified: notified.map(str::to_string),
            auto_tried_at,
            ..Default::default()
        }
    }

    #[test]
    fn compute_due_no_tty() {
        let state = state_builder(Some("v0.2.0"), None, 0);
        assert!(
            compute_due(&state, crate::selfupdate::Version(0, 1, 0), 0, false, false).is_none()
        );
    }

    #[test]
    fn compute_due_latest_absent() {
        let state = state_builder(None, None, 0);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), 0, false, true).is_none());
    }

    #[test]
    fn compute_due_latest_equals_current() {
        let state = state_builder(Some("v0.1.0"), None, 0);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), 0, false, true).is_none());
    }

    #[test]
    fn compute_due_latest_older() {
        let state = state_builder(Some("v0.0.9"), None, 0);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), 0, false, true).is_none());
    }

    #[test]
    fn compute_due_notice_only() {
        // Newer, not notified, auto_tried_at = now → notice only
        let state = state_builder(Some("v0.2.0"), None, 1000);
        let due = compute_due(
            &state,
            crate::selfupdate::Version(0, 1, 0),
            1000,
            false,
            true,
        );
        assert_eq!(
            due,
            Some(Due {
                notice: true,
                auto: false,
                latest: "v0.2.0".into(),
            })
        );
    }

    #[test]
    fn compute_due_auto_only() {
        // Newer, notified, auto_tried_at = 0 → auto only (notice false)
        let state = state_builder(Some("v0.2.0"), Some("v0.2.0"), 0);
        let due = compute_due(
            &state,
            crate::selfupdate::Version(0, 1, 0),
            100_000,
            false,
            true,
        );
        assert_eq!(
            due,
            Some(Due {
                notice: false,
                auto: true,
                latest: "v0.2.0".into(),
            })
        );
    }

    #[test]
    fn compute_due_auto_tried_recent() {
        // Newer, notified, auto_tried_at recent → None
        let state = state_builder(Some("v0.2.0"), Some("v0.2.0"), 900);
        assert!(
            compute_due(
                &state,
                crate::selfupdate::Version(0, 1, 0),
                1000,
                false,
                true
            )
            .is_none()
        );
    }

    #[test]
    fn compute_due_brew_no_auto() {
        // Brew + notified → None (no auto for brew)
        let state = state_builder(Some("v0.2.0"), Some("v0.2.0"), 0);
        assert!(
            compute_due(
                &state,
                crate::selfupdate::Version(0, 1, 0),
                1000,
                true,
                true
            )
            .is_none()
        );
    }

    #[test]
    fn compute_due_brew_notice_only() {
        // Brew + not notified → notice only
        let state = state_builder(Some("v0.2.0"), None, 0);
        let due = compute_due(
            &state,
            crate::selfupdate::Version(0, 1, 0),
            1000,
            true,
            true,
        );
        assert_eq!(
            due,
            Some(Due {
                notice: true,
                auto: false,
                latest: "v0.2.0".into(),
            })
        );
    }

    #[test]
    fn compute_due_latest_unparseable() {
        let state = state_builder(Some("not-a-version"), None, 0);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), 0, false, true).is_none());
    }

    #[test]
    fn notice_for_unwanted_is_silent() {
        // Due for a notice, but the caller does not want one: nothing.
        let due = Due {
            notice: true,
            auto: false,
            latest: "v0.2.0".into(),
        };
        assert!(notice_for(&due, false, "0.1.0", false).is_none());
    }

    #[test]
    fn notice_for_not_due_is_silent() {
        // The caller wants one, but the release is not due: nothing.
        let due = Due {
            notice: false,
            auto: true,
            latest: "v0.2.0".into(),
        };
        assert!(notice_for(&due, true, "0.1.0", false).is_none());
    }

    #[test]
    fn notice_for_binary_install() {
        let due = Due {
            notice: true,
            auto: false,
            latest: "v0.2.0".into(),
        };
        // `latest` is the tag (v-prefixed); `current_version` is the bare
        // CARGO_PKG_VERSION — the format string supplies its v.
        let notice = notice_for(&due, true, "0.1.0", false).expect("notice");
        assert!(notice.contains("v0.2.0"), "the tag: {notice}");
        assert!(
            notice.contains("running v0.1.0"),
            "running + current: {notice}"
        );
        assert!(notice.ends_with(" — update with: ff update"), "{notice}");
    }

    #[test]
    fn notice_for_brew_install() {
        let due = Due {
            notice: true,
            auto: false,
            latest: "v0.2.0".into(),
        };
        let notice = notice_for(&due, true, "0.1.0", true).expect("notice");
        assert!(
            notice.ends_with(" — update with: brew upgrade fufu"),
            "{notice}"
        );
    }

    // ------------------------------------------------------------------
    // check_status_from matrix — pure decision logic, no IO
    // ------------------------------------------------------------------

    #[test]
    fn check_status_unofficial_wins() {
        let state = state_builder(Some("v1.0.0"), None, 0);
        assert_eq!(
            check_status_from(false, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::Unofficial
        );
    }

    #[test]
    fn check_status_no_latest() {
        let state = state_builder(None, None, 0);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::NoCheckYet
        );
    }

    #[test]
    fn check_status_unparseable_latest() {
        let state = state_builder(Some("gibberish"), None, 0);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::NoCheckYet
        );
    }

    #[test]
    fn check_status_available() {
        let state = state_builder(Some("v0.2.0"), None, 0);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::Available("v0.2.0".into())
        );
    }

    #[test]
    fn check_status_up_to_date() {
        let state = state_builder(Some("v0.1.0"), None, 0);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::UpToDate
        );
    }

    #[test]
    fn check_status_no_current() {
        let state = state_builder(Some("v0.2.0"), None, 0);
        assert_eq!(
            check_status_from(true, &state, None),
            CheckStatus::NoCheckYet
        );
    }
}
