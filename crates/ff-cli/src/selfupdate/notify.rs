//! The passive lane's state file; the spawn/pending machinery; the cadence
//! grammar has been extracted to [`crate::cadence`].
//!
//! The lane covers fufu and every declared extension the gate in
//! [`crate::selfupdate::release_repo`] lets through: an official build
//! whose `releases` recipe is a github.com page. One background check per
//! cadence fetches each latest release beside fufu's own, the state file
//! keeps a `latest` and a `notified` per extension beside fufu's, and the
//! notice is one line per stale binary, each release announced at most
//! once. The gates are fufu's own and are not duplicated: an official
//! build of fufu, not CI, a terminal, and `fufu.updateCheck` not off.

use std::collections::BTreeMap;
use std::io::IsTerminal;

use serde::{Deserialize, Serialize};

/// update.json — all timestamps are unix seconds. `interval_secs` caches the
/// parsed `fufu.updateCheck` so the hot path is one file read, no config load:
/// 0 = unset/default, -1 = disabled, else seconds. `extensions` is one
/// entry per declared extension the lane checks, keyed by name, and absent
/// when there are none, so a file an older fufu wrote reads as it did and
/// one this fufu writes reads under an older fufu, which ignores the key.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    pub checked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<String>,
    pub interval_secs: i64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, ExtensionState>,
}

/// One declared extension's half of the state file: the latest release
/// tag the check read, and the one the notice last announced. The same
/// pair fufu keeps for itself; the check's timestamp is shared, because
/// one check covers everything.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<String>,
}

/// Path to the passive-lane state file (`<cache_root>/fufu/update.json`).
pub fn state_path() -> Option<std::path::PathBuf> {
    let root = crate::userdirs::cache_root()?;
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

/// Spawn a detached process (all stdio nulled, cwd inherited). One caller:
/// the daily `update --check` cache refresh.
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

/// Result of the passive decision core — the one action there is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Due {
    pub notice: bool, // a not-yet-announced newer release exists
    pub latest: String,
}

/// The pending() decision, minus all IO. None = fast path, nothing due.
pub(crate) fn compute_due(
    state: &UpdateState,
    current: crate::selfupdate::Version,
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
    if state.notified.as_deref() == state.latest.as_deref() {
        return None;
    }
    Some(Due {
        notice: true,
        latest: latest.clone(),
    })
}

/// The pending() notice, minus all IO. None = nothing to say to the caller.
/// The tail names whatever owns this binary, the same four channels
/// `ff update` dispatches over.
pub(crate) fn notice_for(
    due: &Due,
    want_notice: bool,
    current_version: &str,
    kind: crate::selfupdate::InstallKind,
) -> Option<String> {
    use crate::selfupdate::InstallKind;
    if !due.notice || !want_notice {
        return None;
    }
    let suffix = match kind {
        InstallKind::Script => " — update with: ff update".to_string(),
        InstallKind::Homebrew => {
            format!(" — update with: {}", crate::selfupdate::BREW_UPGRADE)
        }
        InstallKind::Source => {
            format!(" — update with: {}", crate::selfupdate::CARGO_INSTALL)
        }
        InstallKind::Unmanaged => format!(" — see {}", crate::selfupdate::RELEASES_URL),
    };
    Some(format!(
        "ff: {} is available (running v{}){}",
        due.latest, current_version, suffix
    ))
}

/// The extension half of [`compute_due`], minus all IO: the state file's
/// entry for `name` against the version its record carries. `None` when
/// there is nothing to say — no entry, a tag or a recorded version that
/// does not read as one, nothing newer, or a release already announced.
///
/// The recorded version rather than the binary's own, because this runs on
/// every verb and a handshake is a spawn. The record follows the binary
/// through `ff extension <name>` and `ff hook -u`, which is what the doctor
/// row is there to keep true.
pub(crate) fn extension_due(
    state: &UpdateState,
    name: &str,
    recorded_version: &str,
    tty: bool,
) -> Option<Due> {
    if !tty {
        return None;
    }
    let entry = state.extensions.get(name)?;
    let latest = entry.latest.as_ref()?;
    let latest_ver = crate::selfupdate::parse_release(latest)?;
    let current = crate::selfupdate::parse_semver(recorded_version)?;
    if latest_ver <= current {
        return None;
    }
    if entry.notified.as_deref() == Some(latest.as_str()) {
        return None;
    }
    Some(Due {
        notice: true,
        latest: latest.clone(),
    })
}

/// The extension half of [`notice_for`], minus all IO. The tail names the
/// block's recipe for the channel the binary sits on, the way `ff update`
/// would print it: the install recipe is `ff update`, which runs it; a
/// brew recipe is the command; anything else, a channel the block has no
/// recipe for included, is the releases page, which the gate guarantees
/// is there.
pub(crate) fn extension_notice_for(
    due: &Due,
    want_notice: bool,
    name: &str,
    recorded_version: &str,
    kind: crate::selfupdate::InstallKind,
    block: &crate::manifest::Update,
) -> Option<String> {
    use crate::selfupdate::InstallKind;
    if !due.notice || !want_notice {
        return None;
    }
    let recipe = crate::selfupdate::recipe_for(block, kind);
    let suffix = match (kind, recipe) {
        (InstallKind::Script, Some(_)) => " — update with: ff update".to_string(),
        (InstallKind::Homebrew, Some(how)) => format!(" — update with: {how}"),
        _ => format!(" — see {}", block.releases.as_deref().unwrap_or_default()),
    };
    Some(format!(
        "ff: ff-{name} {} is available (running {recorded_version}){suffix}",
        due.latest
    ))
}

/// The background check's extension half: one lookup per declared
/// extension the gate lets through, with the lookup injected so the
/// refresh is testable without a network. A lookup that answers replaces
/// `latest` and keeps `notified`; one that fails leaves the entry as it
/// was. Entries for extensions no longer declared, or no longer eligible,
/// are dropped, so the file follows the registry.
pub(crate) fn refresh_extensions(
    state: &mut UpdateState,
    declared: &[crate::registry::Declared],
    lookup: impl Fn(&str) -> Option<String>,
) {
    let mut kept: BTreeMap<String, ExtensionState> = BTreeMap::new();
    for extension in declared {
        let Some(repo) = crate::selfupdate::release_repo(&extension.manifest) else {
            continue;
        };
        let name = extension.name();
        let mut entry = state.extensions.remove(name).unwrap_or_default();
        if let Some(tag) = lookup(&repo)
            && crate::selfupdate::parse_release(&tag).is_some()
        {
            entry.latest = Some(tag);
        }
        kept.insert(name.to_string(), entry);
    }
    state.extensions = kept;
}

/// One line the passive lane has to say, and whose release it announces:
/// fufu's own when `about` is `None`, else the named extension's. What
/// [`pending`] hands back and [`mark_notified`] spends.
#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub line: String,
    pub about: Option<String>,
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

/// The release notices pending: fufu's own first, then one per declared
/// extension whose latest release is ahead of its record, in registry
/// order. Empty when there is nothing to say.
///
/// This lane never installs anything: it notices, and names the command
/// that would.
///
/// The fast path is one file read. The registry is read only when the
/// file carries an extension entry, and an extension's binary is found by
/// a PATH scan rather than a handshake: nothing here spawns.
pub fn pending(
    repo: &ff_core::gix::Repository,
    current_version: &str,
    want_notice: bool,
) -> Vec<Notice> {
    if !gates_open() {
        return Vec::new();
    }
    let Some(path) = state_path() else {
        return Vec::new();
    };
    let state = load_state(&path);
    let tty = std::io::stderr().is_terminal();

    let fufu = crate::selfupdate::parse_semver(current_version)
        .and_then(|current| compute_due(&state, current, tty));
    let extensions: Vec<(&crate::registry::Declared, Due)> = if state.extensions.is_empty() {
        Vec::new()
    } else {
        crate::registry::read()
            .declared()
            .iter()
            .filter_map(|declared| {
                extension_due(&state, declared.name(), &declared.manifest.version, tty)
                    .map(|due| (declared, due))
            })
            .collect()
    };
    if fufu.is_none() && extensions.is_empty() {
        return Vec::new();
    }

    // Something is due — NOW check live config.
    if crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.updateCheck") == -1 {
        return Vec::new();
    }

    let mut notices = Vec::new();
    if let Some(due) = fufu {
        // An unresolvable exe cannot be classified; the releases page is the
        // answer that is true for every install.
        let kind = std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .map(|exe| crate::selfupdate::classify_install(&exe, true))
            .unwrap_or(crate::selfupdate::InstallKind::Unmanaged);
        if let Some(line) = notice_for(&due, want_notice, current_version, kind) {
            notices.push(Notice { line, about: None });
        }
    }
    for (declared, due) in extensions {
        // A binary that has left PATH has nothing to announce; the record
        // is doctor's to report, and the release waits for a binary.
        let Some(exe) = declared.resolve() else {
            continue;
        };
        let Some(block) = &declared.manifest.update else {
            continue;
        };
        let exe = exe.canonicalize().unwrap_or(exe);
        let bin = crate::selfupdate::extension_bin_dir(block.bin.as_deref()).unwrap_or_default();
        let kind =
            crate::selfupdate::classify_extension_at(&exe, true, block.install.is_some(), &bin);
        if let Some(line) = extension_notice_for(
            &due,
            want_notice,
            declared.name(),
            &declared.manifest.version,
            kind,
            block,
        ) {
            notices.push(Notice {
                line,
                about: Some(declared.name().to_string()),
            });
        }
    }
    notices
}

/// Mark what was just printed as notified — a release announces at most
/// once, ever, and only the releases whose lines were printed are spent.
pub fn mark_notified(notices: &[Notice]) {
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    mark_notified_in(&mut state, notices);
    let _ = save_state(&path, &state);
}

/// [`mark_notified`] against a state in hand: fufu's `notified` follows
/// its `latest` for a notice about fufu, and an extension's follows its
/// own for a notice about it.
pub(crate) fn mark_notified_in(state: &mut UpdateState, notices: &[Notice]) {
    for notice in notices {
        match &notice.about {
            None => state.notified = state.latest.clone(),
            Some(name) => {
                if let Some(entry) = state.extensions.get_mut(name) {
                    entry.notified = entry.latest.clone();
                }
            }
        }
    }
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
            interval_secs: 86_400,
            extensions: BTreeMap::new(),
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);
        // With no extension entries the file is the one an older fufu
        // wrote: no key at all.
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(!body.contains("extensions"), "{body}");

        // And with entries, they round-trip beside fufu's own.
        let mut state = state;
        state.extensions.insert(
            "tower".into(),
            ExtensionState {
                latest: Some("v0.5.0".into()),
                notified: None,
            },
        );
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);
    }

    /// A file an older fufu wrote has no `extensions` key and reads as it
    /// did; one with a key an older fufu never wrote reads too.
    #[test]
    fn an_old_cache_reads_with_no_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.json");
        std::fs::write(
            &path,
            r#"{"checked_at":1700000000,"latest":"v0.2.0","notified":"v0.2.0","interval_secs":86400}"#,
        )
        .unwrap();
        let state = load_state(&path);
        assert_eq!(state.latest.as_deref(), Some("v0.2.0"));
        assert!(state.extensions.is_empty());

        std::fs::write(
            &path,
            r#"{"checked_at":1700000000,"interval_secs":0,"extensions":{"tower":{"latest":"v0.5.0"}}}"#,
        )
        .unwrap();
        let state = load_state(&path);
        assert_eq!(
            state.extensions["tower"],
            ExtensionState {
                latest: Some("v0.5.0".into()),
                notified: None
            }
        );
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

    fn state_builder(latest: Option<&str>, notified: Option<&str>) -> UpdateState {
        UpdateState {
            latest: latest.map(str::to_string),
            notified: notified.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn compute_due_no_tty() {
        let state = state_builder(Some("v0.2.0"), None);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), false).is_none());
    }

    #[test]
    fn compute_due_latest_absent() {
        let state = state_builder(None, None);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), true).is_none());
    }

    #[test]
    fn compute_due_latest_equals_current() {
        let state = state_builder(Some("v0.1.0"), None);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), true).is_none());
    }

    #[test]
    fn compute_due_latest_older() {
        let state = state_builder(Some("v0.0.9"), None);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), true).is_none());
    }

    #[test]
    fn compute_due_notice() {
        let state = state_builder(Some("v0.2.0"), None);
        assert_eq!(
            compute_due(&state, crate::selfupdate::Version(0, 1, 0), true),
            Some(Due {
                notice: true,
                latest: "v0.2.0".into(),
            })
        );
    }

    #[test]
    fn compute_due_already_notified() {
        // A release announces at most once, ever.
        let state = state_builder(Some("v0.2.0"), Some("v0.2.0"));
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), true).is_none());
    }

    #[test]
    fn compute_due_latest_unparseable() {
        let state = state_builder(Some("not-a-version"), None);
        assert!(compute_due(&state, crate::selfupdate::Version(0, 1, 0), true).is_none());
    }

    #[test]
    fn notice_for_unwanted_is_silent() {
        // Due for a notice, but the caller does not want one: nothing.
        let due = Due {
            notice: true,
            latest: "v0.2.0".into(),
        };
        assert!(notice_for(&due, false, "0.1.0", crate::selfupdate::InstallKind::Script).is_none());
    }

    #[test]
    fn notice_for_not_due_is_silent() {
        // The caller wants one, but the release is not due: nothing.
        let due = Due {
            notice: false,
            latest: "v0.2.0".into(),
        };
        assert!(notice_for(&due, true, "0.1.0", crate::selfupdate::InstallKind::Script).is_none());
    }

    #[test]
    fn notice_for_script_install() {
        let due = Due {
            notice: true,
            latest: "v0.2.0".into(),
        };
        // `latest` is the tag (v-prefixed); `current_version` is the bare
        // CARGO_PKG_VERSION — the format string supplies its v.
        let notice = notice_for(&due, true, "0.1.0", crate::selfupdate::InstallKind::Script)
            .expect("notice");
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
            latest: "v0.2.0".into(),
        };
        let notice = notice_for(
            &due,
            true,
            "0.1.0",
            crate::selfupdate::InstallKind::Homebrew,
        )
        .expect("notice");
        assert!(
            notice.ends_with(" — update with: brew upgrade fufu"),
            "{notice}"
        );
    }

    #[test]
    fn notice_for_unmanaged_install() {
        // Nothing fufu can name updates a binary mise or nix placed, so the
        // tail is the page a person goes to rather than a command to run.
        let due = Due {
            notice: true,
            latest: "v0.2.0".into(),
        };
        let notice = notice_for(
            &due,
            true,
            "0.1.0",
            crate::selfupdate::InstallKind::Unmanaged,
        )
        .expect("notice");
        assert!(
            notice.ends_with(" — see https://github.com/tyler-johnson/fufu/releases/latest"),
            "{notice}"
        );
    }

    // ------------------------------------------------------------------
    // check_status_from matrix — pure decision logic, no IO
    // ------------------------------------------------------------------

    #[test]
    fn check_status_unofficial_wins() {
        let state = state_builder(Some("v1.0.0"), None);
        assert_eq!(
            check_status_from(false, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::Unofficial
        );
    }

    #[test]
    fn check_status_no_latest() {
        let state = state_builder(None, None);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::NoCheckYet
        );
    }

    #[test]
    fn check_status_unparseable_latest() {
        let state = state_builder(Some("gibberish"), None);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::NoCheckYet
        );
    }

    #[test]
    fn check_status_available() {
        let state = state_builder(Some("v0.2.0"), None);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::Available("v0.2.0".into())
        );
    }

    #[test]
    fn check_status_up_to_date() {
        let state = state_builder(Some("v0.1.0"), None);
        assert_eq!(
            check_status_from(true, &state, Some(crate::selfupdate::Version(0, 1, 0))),
            CheckStatus::UpToDate
        );
    }

    #[test]
    fn check_status_no_current() {
        let state = state_builder(Some("v0.2.0"), None);
        assert_eq!(
            check_status_from(true, &state, None),
            CheckStatus::NoCheckYet
        );
    }

    // ------------------------------------------------------------------
    // the extension half — the same pure cores over the per-extension
    // entries, and the refresh with the lookup injected
    // ------------------------------------------------------------------

    const RELEASES: &str = "https://github.com/tyler-johnson/tower/releases/latest";
    const INSTALL: &str = "https://raw.githubusercontent.com/tyler-johnson/tower/main/install.sh";

    fn ext_state(name: &str, latest: Option<&str>, notified: Option<&str>) -> UpdateState {
        let mut state = UpdateState::default();
        state.extensions.insert(
            name.into(),
            ExtensionState {
                latest: latest.map(str::to_string),
                notified: notified.map(str::to_string),
            },
        );
        state
    }

    /// A declared extension with the `update` block and `build` given as
    /// JSON, recorded at a version.
    fn declared(
        name: &str,
        version: &str,
        update: serde_json::Value,
        build: Option<&str>,
    ) -> crate::registry::Declared {
        let mut value = serde_json::json!({
            "name": name,
            "version": version,
            "contract": crate::machine::CONTRACT,
            "verbs": [{"name": "board", "read_only": true}],
            "undoable": true,
        });
        if !update.is_null() {
            value["update"] = update;
        }
        if let Some(build) = build {
            value["build"] = serde_json::Value::String(build.into());
        }
        crate::registry::Declared {
            manifest: crate::manifest::parse(value).expect("a manifest the page types"),
            path: std::path::PathBuf::from(format!("/usr/local/bin/ff-{name}")),
            declared_at: 1,
        }
    }

    fn full_block() -> serde_json::Value {
        serde_json::json!({"brew": "tyler-johnson/tap/tower", "install": INSTALL, "releases": RELEASES})
    }

    /// A stale extension is due once: the line is due, marking it spent
    /// makes it not due, and a newer release after that is due again.
    #[test]
    fn a_stale_extension_is_due_once() {
        let mut state = ext_state("tower", Some("v0.5.0"), None);
        assert_eq!(
            extension_due(&state, "tower", "0.4.1", true),
            Some(Due {
                notice: true,
                latest: "v0.5.0".into()
            })
        );
        mark_notified_in(
            &mut state,
            &[Notice {
                line: String::new(),
                about: Some("tower".into()),
            }],
        );
        assert_eq!(
            state.extensions["tower"].notified.as_deref(),
            Some("v0.5.0")
        );
        assert!(extension_due(&state, "tower", "0.4.1", true).is_none());

        state.extensions.get_mut("tower").unwrap().latest = Some("v0.6.0".into());
        assert_eq!(
            extension_due(&state, "tower", "0.4.1", true)
                .expect("due again")
                .latest,
            "v0.6.0"
        );
    }

    /// Marking fufu's own notice spends fufu's and no extension's, and the
    /// other way round.
    #[test]
    fn marking_spends_only_what_was_printed() {
        let mut state = ext_state("tower", Some("v0.5.0"), None);
        state.latest = Some("v0.2.0".into());
        mark_notified_in(
            &mut state,
            &[Notice {
                line: String::new(),
                about: None,
            }],
        );
        assert_eq!(state.notified.as_deref(), Some("v0.2.0"));
        assert_eq!(state.extensions["tower"].notified, None);
        // A name with no entry is nothing to mark.
        mark_notified_in(
            &mut state,
            &[Notice {
                line: String::new(),
                about: Some("bay".into()),
            }],
        );
        assert!(!state.extensions.contains_key("bay"));
    }

    #[test]
    fn an_extension_with_nothing_newer_is_not_due() {
        // No entry at all.
        assert!(extension_due(&UpdateState::default(), "tower", "0.4.1", true).is_none());
        // An entry with no latest yet.
        assert!(extension_due(&ext_state("tower", None, None), "tower", "0.4.1", true).is_none());
        // Level, and behind.
        assert!(
            extension_due(
                &ext_state("tower", Some("v0.4.1"), None),
                "tower",
                "0.4.1",
                true
            )
            .is_none()
        );
        assert!(
            extension_due(
                &ext_state("tower", Some("v0.4.0"), None),
                "tower",
                "0.4.1",
                true
            )
            .is_none()
        );
        // No terminal.
        assert!(
            extension_due(
                &ext_state("tower", Some("v0.5.0"), None),
                "tower",
                "0.4.1",
                false
            )
            .is_none()
        );
        // A recorded version that does not read as one, and a tag that
        // does not either.
        assert!(
            extension_due(
                &ext_state("tower", Some("v0.5.0"), None),
                "tower",
                "0.4.1-dev",
                true
            )
            .is_none()
        );
        assert!(
            extension_due(
                &ext_state("tower", Some("nightly"), None),
                "tower",
                "0.4.1",
                true
            )
            .is_none()
        );
        // A bare tag is a version too.
        assert!(
            extension_due(
                &ext_state("tower", Some("0.5.0"), None),
                "tower",
                "0.4.1",
                true
            )
            .is_some()
        );
    }

    /// The line names the extension, both versions, and the recipe for
    /// the channel the way `ff update` would print it.
    #[test]
    fn an_extension_notice_names_its_recipe() {
        use crate::selfupdate::InstallKind;
        let due = Due {
            notice: true,
            latest: "v0.5.0".into(),
        };
        let block = declared("tower", "0.4.1", full_block(), None)
            .manifest
            .update
            .unwrap();
        let line = |kind| extension_notice_for(&due, true, "tower", "0.4.1", kind, &block);
        assert_eq!(
            line(InstallKind::Script).as_deref(),
            Some("ff: ff-tower v0.5.0 is available (running 0.4.1) — update with: ff update")
        );
        assert_eq!(
            line(InstallKind::Homebrew).as_deref(),
            Some(
                "ff: ff-tower v0.5.0 is available (running 0.4.1) — update with: brew upgrade tyler-johnson/tap/tower"
            )
        );
        assert_eq!(
            line(InstallKind::Unmanaged).as_deref(),
            Some(&*format!(
                "ff: ff-tower v0.5.0 is available (running 0.4.1) — see {RELEASES}"
            ))
        );
        // A channel the block has no recipe for falls back to the page.
        let page_only = declared(
            "tower",
            "0.4.1",
            serde_json::json!({"releases": RELEASES}),
            None,
        )
        .manifest
        .update
        .unwrap();
        assert_eq!(
            extension_notice_for(
                &due,
                true,
                "tower",
                "0.4.1",
                InstallKind::Homebrew,
                &page_only
            )
            .as_deref(),
            Some(&*format!(
                "ff: ff-tower v0.5.0 is available (running 0.4.1) — see {RELEASES}"
            ))
        );
        // Not wanted, or not due: nothing.
        assert!(
            extension_notice_for(&due, false, "tower", "0.4.1", InstallKind::Script, &block)
                .is_none()
        );
        let not_due = Due {
            notice: false,
            latest: "v0.5.0".into(),
        };
        assert!(
            extension_notice_for(
                &not_due,
                true,
                "tower",
                "0.4.1",
                InstallKind::Script,
                &block
            )
            .is_none()
        );
    }

    /// The refresh looks up an official build with a github.com page and
    /// nothing else: a `source` build, a manifest with no `releases`
    /// recipe, and a page on another host each get no lookup and no entry.
    #[test]
    fn the_refresh_checks_only_what_the_gate_lets_through() {
        let extensions = [
            declared("tower", "0.4.1", full_block(), None),
            declared("built", "1.0.0", full_block(), Some("source")),
            declared(
                "brewed",
                "1.0.0",
                serde_json::json!({"brew": "tyler-johnson/tap/brewed"}),
                None,
            ),
            declared(
                "elsewhere",
                "1.0.0",
                serde_json::json!({"releases": "https://gitlab.com/tyler-johnson/elsewhere/-/releases"}),
                None,
            ),
            declared("bare", "1.0.0", serde_json::Value::Null, None),
        ];
        let asked = std::cell::RefCell::new(Vec::new());
        let mut state = UpdateState::default();
        refresh_extensions(&mut state, &extensions, |repo| {
            asked.borrow_mut().push(repo.to_string());
            Some("v0.5.0".into())
        });
        assert_eq!(*asked.borrow(), ["tyler-johnson/tower"]);
        let names: Vec<&String> = state.extensions.keys().collect();
        assert_eq!(names, ["tower"]);
        assert_eq!(state.extensions["tower"].latest.as_deref(), Some("v0.5.0"));
        assert!(extension_due(&state, "built", "1.0.0", true).is_none());
        assert!(extension_due(&state, "brewed", "1.0.0", true).is_none());
        assert!(extension_due(&state, "elsewhere", "1.0.0", true).is_none());
    }

    /// A lookup that fails leaves the entry as it was, one that answers a
    /// tag that is not a version leaves `latest` alone, and `notified`
    /// survives both. An entry for an extension no longer declared goes.
    #[test]
    fn the_refresh_keeps_what_it_cannot_replace_and_drops_what_is_gone() {
        let mut state = ext_state("tower", Some("v0.5.0"), Some("v0.5.0"));
        state.extensions.insert(
            "removed".into(),
            ExtensionState {
                latest: Some("v1.0.0".into()),
                notified: None,
            },
        );
        let extensions = [declared("tower", "0.4.1", full_block(), None)];

        refresh_extensions(&mut state, &extensions, |_| None);
        assert_eq!(
            state.extensions["tower"],
            ExtensionState {
                latest: Some("v0.5.0".into()),
                notified: Some("v0.5.0".into()),
            }
        );
        assert!(!state.extensions.contains_key("removed"));

        refresh_extensions(&mut state, &extensions, |_| Some("nightly".into()));
        assert_eq!(state.extensions["tower"].latest.as_deref(), Some("v0.5.0"));

        refresh_extensions(&mut state, &extensions, |_| Some("v0.6.0".into()));
        assert_eq!(
            state.extensions["tower"],
            ExtensionState {
                latest: Some("v0.6.0".into()),
                notified: Some("v0.5.0".into()),
            }
        );
        assert!(extension_due(&state, "tower", "0.4.1", true).is_some());
    }
}
