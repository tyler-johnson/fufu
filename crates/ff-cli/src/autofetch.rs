//! The fetch lane. Every verb that reads the tracking refs — `ff status`
//! counting against the copy, `ff switch` looking for a branch a teammate
//! pushed, `ff branch` listing what the remote holds — reads refs as old as
//! the last fetch, and git's answer is that everyone remembers to fetch.
//! fufu's is that nobody has to ask whether they are up to date: at most
//! once per `fufu.autoFetch` (ten minutes by default), per repository, a
//! fetch rides an `ff` command inline before its verb runs, so the verb
//! reads fresh refs. The fetch is `net::fetch`'s, under a deadline and with
//! every prompt off, and it prunes what the remote no longer holds.
//!
//! On autotrim's shape: a stamp file, one file read on the hot path, config
//! consulted only when the stamp says a fetch might be due, the stamp
//! written before the wire so a fetch that hangs to its deadline is not
//! retried until the cadence is up. One stamp per repository rather than
//! per chain, because the tracking refs are shared.
//!
//! What the lane never does is fail the verb. `maybe_fetch` consumes its own
//! `Result`: a remote that is down costs a bounded wait and one dim line,
//! recorded in the stamp for `ff doctor` to report, and the verb answers
//! from the refs as they stand.

use std::io::IsTerminal;

use serde::{Deserialize, Serialize};

use crate::cli::Fetch;
use crate::ctx::Ctx;

/// The default cadence: ten minutes.
pub const DEFAULT_SECS: i64 = 600;

/// The lane's deadline, in seconds. Three is long enough for a healthy
/// remote to answer and short enough that a reader behind a dead one is
/// still a reader.
const DEADLINE_SECS: u64 = 3;

/// autofetch.json — `fetched_at` is when a fetch last started, stamped
/// before the wire. `interval_secs` caches the parsed `fufu.autoFetch` so
/// staleness is decided from the file alone: 0 = never read (default
/// cadence), -1 = disabled, else seconds. `remote` is the one last fetched
/// from; `last_error` the first useful line of the last failure, cleared by
/// a success; `failed_since` the time of the first failure in the current
/// run of them, 0 while healthy.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchState {
    pub fetched_at: i64,
    pub interval_secs: i64,
    pub remote: String,
    pub last_error: Option<String>,
    pub failed_since: i64,
}

/// Path to this repository's auto-fetch state file.
fn state_path(repo: &ff_core::gix::Repository) -> std::path::PathBuf {
    repo.common_dir().join("fufu").join("autofetch.json")
}

/// Load the auto-fetch state from the repo.
///
/// Any error (missing, unreadable, corrupt JSON) yields [`FetchState::default`].
pub fn load(repo: &ff_core::gix::Repository) -> FetchState {
    std::fs::read(state_path(repo))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Save the auto-fetch state using atomic temp-file + rename.
fn save(repo: &ff_core::gix::Repository, state: &FetchState) -> std::io::Result<()> {
    let path = state_path(repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // One temp file per destination, the rule autotrim states: two
    // worktrees share the `fufu` directory, and a shared temp name would
    // let one rename steal the other's half-written stamp.
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let tmp = path.with_file_name(format!("{name}.ff-tmp"));
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
pub(crate) fn due_by_cache(state: &FetchState, now: i64) -> bool {
    now - state.fetched_at >= crate::cadence::stale_after_with(state.interval_secs, DEFAULT_SECS)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Disabled,
    Fresh,
    Fetch,
}

/// Phase-2 decision, once the live setting has been read and encoded.
pub(crate) fn due_by_live(encoded: i64, fetched_at: i64, now: i64) -> Lane {
    match crate::cadence::effective_with(encoded, DEFAULT_SECS) {
        None => Lane::Disabled,
        Some(interval) => {
            if now - fetched_at < interval {
                Lane::Fresh
            } else {
                Lane::Fetch
            }
        }
    }
}

/// The remote the lane fetches from: the one the branch underfoot answers
/// to, by fufu's own ladder; for a detached head the repository default,
/// then `origin` if it is configured. `None` — no remote, or more than one
/// and nothing to choose by — means the lane stamps nothing and returns.
fn remote_for(repo: &ff_core::gix::Repository) -> Option<String> {
    use ff_core::gix::remote::Direction;
    let head = repo.head_name().ok().flatten();
    if let Some(name) = head {
        return match ff_core::remote::for_branch(repo, name.shorten().to_string().as_str()) {
            ff_core::remote::RemoteChoice::Named(remote) => Some(remote),
            ff_core::remote::RemoteChoice::NoneConfigured
            | ff_core::remote::RemoteChoice::Ambiguous { .. } => None,
        };
    }
    if let Some(name) = repo.remote_default_name(Direction::Fetch) {
        return Some(name.to_string());
    }
    repo.remote_names()
        .iter()
        .map(|name| name.to_string())
        .find(|name| name == "origin")
}

/// Maybe fetch for this repository, before the verb reads its tracking refs.
///
/// Never errors, never propagates the fetch's failure, and says at most two
/// dim lines on a terminal — that it is fetching, and that it could not.
pub fn maybe_fetch(repo: &ff_core::gix::Repository, ctx: &Ctx, lane: Fetch) {
    if ctx.no_fetch {
        return;
    }
    // CI: an ephemeral clone whose refs a checkout just wrote, and a
    // network fufu should not be the one reaching for. `--fetch` is the
    // person asking, and is honored there too.
    if std::env::var_os("CI").is_some() && !ctx.fetch {
        return;
    }
    // Bare repos have no branch underfoot to read the refs for.
    if repo.workdir().is_none() {
        return;
    }
    let Some(remote) = remote_for(repo) else {
        return;
    };

    let mut state = load(repo);
    let now = now_secs();

    // Hot path: one file read, nothing else. `--fetch` and doctor's
    // every-run lane read past it to the live setting.
    let on_cadence = !ctx.fetch && lane == Fetch::Cadence;
    if on_cadence && !due_by_cache(&state, now) {
        return;
    }

    // The stamp says a fetch might be due — read the live setting. `false`
    // turns the lane off for the cadence and for doctor alike; only `--fetch`
    // runs past it, since that is the person asking.
    state.interval_secs =
        crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.autoFetch");
    match due_by_live(state.interval_secs, state.fetched_at, now) {
        Lane::Disabled if !ctx.fetch => {
            state.fetched_at = now;
            let _ = save(repo, &state);
            return;
        }
        Lane::Fresh if on_cadence => {
            let _ = save(repo, &state);
            return;
        }
        Lane::Disabled | Lane::Fresh | Lane::Fetch => {}
    }

    // Stamp first — a fetch that hangs to its deadline retries on the
    // cadence, not on every command.
    state.fetched_at = now;
    state.remote = remote.clone();
    let _ = save(repo, &state);

    let speak = !ctx.json && std::io::stderr().is_terminal();
    let colored = crate::pager::color_enabled();
    if speak {
        eprintln!(
            "{}",
            crate::render::paint_dim(&format!("fetching from {remote}"), colored)
        );
    }

    let Some(cwd) = repo.workdir() else {
        return;
    };
    // The deadline is the cadence's: a fetch the person asked for with
    // `--fetch` waits as long as the remote takes, as pull's does.
    let deadline = on_cadence.then_some(std::time::Duration::from_secs(DEADLINE_SECS));
    let result = crate::net::fetch(
        cwd,
        &remote,
        &crate::net::FetchOptions {
            deadline,
            interactive: false,
            prune: true,
            tags: false,
        },
    );
    if result.is_err() && speak {
        eprintln!(
            "{}",
            crate::render::paint_dim(&format!("{remote} unreachable, fetch skipped"), colored)
        );
    }
    record(repo, state, now, result.as_ref().map(|_| ()));
}

/// Stamp the clock — `ff pull` just fetched, so the lane's next fetch is a
/// cadence away, and its outcome is the stamp's record too.
pub fn stamp(repo: &ff_core::gix::Repository, remote: &str, result: Result<(), &ff_core::Error>) {
    let mut state = load(repo);
    let now = now_secs();
    state.fetched_at = now;
    state.remote = remote.to_string();
    record(repo, state, now, result);
}

/// Write the outcome of one fetch into the stamp: a success clears the
/// failure record, a failure keeps its first line and the time the run of
/// failures began.
fn record(
    repo: &ff_core::gix::Repository,
    mut state: FetchState,
    now: i64,
    result: Result<(), &ff_core::Error>,
) {
    match result {
        Ok(()) => {
            state.last_error = None;
            state.failed_since = 0;
        }
        Err(err) => {
            state.last_error = Some(err.to_string().lines().next().unwrap_or("").to_string());
            if state.failed_since == 0 {
                state.failed_since = now;
            }
        }
    }
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
    fn due_by_cache_never_fetched_is_due() {
        let state = FetchState::default();
        assert!(due_by_cache(&state, 1_000_000));
    }

    #[test]
    fn due_by_cache_fresh_stamp_not_due() {
        let state = FetchState {
            fetched_at: 1000,
            interval_secs: 0, // default = 600
            ..Default::default()
        };
        assert!(!due_by_cache(&state, 1599));
        assert!(due_by_cache(&state, 1600));
    }

    #[test]
    fn due_by_cache_disabled_re_reads_on_the_default_cadence() {
        let state = FetchState {
            fetched_at: 0,
            interval_secs: -1,
            ..Default::default()
        };
        assert!(due_by_cache(&state, 600));
        assert!(!due_by_cache(&state, 599));
    }

    #[test]
    fn due_by_cache_sixty_second_floor() {
        let state = FetchState {
            fetched_at: 0,
            interval_secs: 10,
            ..Default::default()
        };
        assert!(due_by_cache(&state, 60));
        assert!(!due_by_cache(&state, 59));
    }

    #[test]
    fn due_by_live_decides_from_the_ten_minute_default() {
        assert_eq!(due_by_live(-1, 0, 1000), Lane::Disabled);
        assert_eq!(due_by_live(0, 1000, 1599), Lane::Fresh);
        assert_eq!(due_by_live(0, 1000, 1600), Lane::Fetch);
        assert_eq!(due_by_live(7200, 1000, 1600), Lane::Fresh);
    }

    #[test]
    fn a_stamp_round_trips_and_a_corrupt_one_is_the_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = ff_core::gix::init(dir.path()).unwrap();
        let state = FetchState {
            fetched_at: 42,
            interval_secs: 600,
            remote: "origin".into(),
            last_error: Some("fetching from origin failed".into()),
            failed_since: 41,
        };
        save(&repo, &state).unwrap();
        assert_eq!(load(&repo), state);
        std::fs::write(state_path(&repo), "{not json").unwrap();
        assert_eq!(load(&repo), FetchState::default());
    }
}
