//! The automatic trim. Snapshots age out of the `fufu.keep` window whether or
//! not anyone remembers `ff trim`, so retention rides the commands that
//! already run: at most once per `fufu.autoTrim` (daily by default), per
//! repository, a trim runs inline — no child process, because the engine is
//! native and fufu does not spawn. The hot path pays one file read to decide
//! nothing is due; config is consulted only when the stamp says it might be.

use serde::{Deserialize, Serialize};

/// autotrim.json — `trimmed_at` is when a trim last ran (or when the lane last
/// noticed it was disabled). `interval_secs` caches the parsed `fufu.autoTrim`
/// so staleness is decided from the file alone: 0 = never read (default
/// cadence), -1 = disabled, else seconds.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrimState {
    pub trimmed_at: i64,
    pub interval_secs: i64,
}

/// Path to the per-repo auto-trim state file.
fn state_path(repo: &ff_core::gix::Repository) -> std::path::PathBuf {
    repo.common_dir().join("fufu").join("autotrim.json")
}

/// Load the auto-trim state from the repo.
///
/// Any error (missing, unreadable, corrupt JSON) yields [`TrimState::default`].
pub fn load(repo: &ff_core::gix::Repository) -> TrimState {
    std::fs::read(state_path(repo))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Save the auto-trim state using atomic temp-file + rename.
fn save(repo: &ff_core::gix::Repository, state: &TrimState) -> std::io::Result<()> {
    let path = state_path(repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name("autotrim.json.ff-tmp");
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

/// Phase-1 gate, decided from the stamp alone — no config load.
pub(crate) fn due_by_cache(state: &TrimState, now: i64) -> bool {
    now - state.trimmed_at >= crate::cadence::stale_after(state.interval_secs)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Disabled,
    Fresh,
    Trim,
}

/// Phase-2 decision, once the live setting has been read and encoded.
pub(crate) fn due_by_live(encoded: i64, trimmed_at: i64, now: i64) -> Lane {
    match crate::cadence::effective(encoded) {
        None => Lane::Disabled,
        Some(interval) => {
            if now - trimmed_at < interval {
                Lane::Fresh
            } else {
                Lane::Trim
            }
        }
    }
}

/// Maybe run a trim for this repository.
///
/// Never errors, never prints (except under `FF_DEBUG`), returns `()`.
pub fn maybe_trim(repo: &ff_core::gix::Repository) {
    // CI: ephemeral clones are not worth the walk.
    if std::env::var_os("CI").is_some() {
        return;
    }

    // Bare repos have no working tree to protect.
    if repo.workdir().is_none() {
        return;
    }

    let mut state = load(repo);
    let now = now_secs();

    // Hot path: one file read, nothing else.
    if !due_by_cache(&state, now) {
        return;
    }

    // Cached cadence says a trim might be due — read the live setting.
    state.interval_secs =
        crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.autoTrim");

    match due_by_live(state.interval_secs, state.trimmed_at, now) {
        Lane::Disabled => {
            state.trimmed_at = now;
            let _ = save(repo, &state);
            return;
        }
        Lane::Fresh => {
            let _ = save(repo, &state);
            return;
        }
        Lane::Trim => {}
    }

    // Stamp first — a trim that fails retries on the cadence, not on every command.
    state.trimmed_at = now;
    let _ = save(repo, &state);

    // Run the trim inline.
    if let Err(err) = ff_core::trim(
        repo,
        &ff_core::TrimOptions {
            now: Some(now),
            dry_run: false,
            gone: false,
            keep_secs: None,
        },
    ) && std::env::var_os("FF_DEBUG").is_some()
    {
        eprintln!("ff[debug]: auto-trim failed: {err}");
    }
}

/// Stamp the clock — a manual trim just ran, so reset the auto-trim timer.
pub fn stamp(repo: &ff_core::gix::Repository) {
    let mut state = load(repo);
    state.trimmed_at = now_secs();
    let _ = save(repo, &state);
}

/// Write-through: persist a freshly-read cadence encoding to the stamp.
pub fn sync_interval(repo: &ff_core::gix::Repository, encoded: i64) {
    let mut state = load(repo);
    state.interval_secs = encoded;
    let _ = save(repo, &state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_by_cache_never_trimmed_is_due() {
        let state = TrimState {
            trimmed_at: 0,
            interval_secs: 0,
        };
        // Real unix timestamps are >> 86400, so never-trimmed is always due.
        assert!(due_by_cache(&state, 1_000_000));
    }

    #[test]
    fn due_by_cache_fresh_stamp_not_due() {
        let state = TrimState {
            trimmed_at: 1000,
            interval_secs: 0, // default = 86400
        };
        assert!(!due_by_cache(&state, 1001));
    }

    #[test]
    fn due_by_cache_disabled_re_reads_daily() {
        // -1 means disabled, stale_after returns 86400
        let state = TrimState {
            trimmed_at: 0,
            interval_secs: -1,
        };
        assert!(due_by_cache(&state, 86_400));
        assert!(!due_by_cache(&state, 86_399));
    }

    #[test]
    fn due_by_cache_sixty_second_floor() {
        let state = TrimState {
            trimmed_at: 0,
            interval_secs: 10, // below floor
        };
        // stale_after(10) = 10.max(60) = 60
        assert!(due_by_cache(&state, 60));
        assert!(!due_by_cache(&state, 59));
    }

    #[test]
    fn due_by_live_disabled() {
        assert_eq!(due_by_live(-1, 0, 1000), Lane::Disabled);
    }

    #[test]
    fn due_by_live_fresh() {
        // encoded 0 → default 86400s
        assert_eq!(due_by_live(0, 1000, 1001), Lane::Fresh);
    }

    #[test]
    fn due_by_live_trim() {
        // encoded 0 → default 86400s, but 100000 > 86400
        assert_eq!(due_by_live(0, 0, 100_000), Lane::Trim);
    }
}
