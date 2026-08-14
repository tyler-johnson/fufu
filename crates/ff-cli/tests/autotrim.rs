//! The auto-trim lane: drives the real `ff` binary against hermetic fixtures to
//! prove the lane's staleness decisions and its silence.

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use ff_testsupport::fixtures::{Fixture, null_device};

// ── Runner ────────────────────────────────────────────────────────────────

/// Run `ff` in a fixture repo with the auto-trim lane active.
fn ff(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        // Real CI sets CI, and the lane skips when it is set — remove so the
        // lane actually runs in our test harness.
        .env_remove("CI")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
        .output()
        .expect("spawn ff")
}

/// Load the autotrim.json stamp (default if missing/corrupt).
fn load_stamp(fx: &Fixture) -> serde_json::Value {
    let path = fx.path().join(".git/fufu/autotrim.json");
    if path.exists() {
        let data = std::fs::read_to_string(&path).expect("read stamp");
        serde_json::from_str(&data).expect("stamp is json")
    } else {
        serde_json::json!({"trimmed_at": 0, "interval_secs": 0})
    }
}

/// Overwrite the stamp file with a due stamp (`trimmed_at: 0`, `interval_secs: 0`).
fn write_due_stamp(fx: &Fixture) {
    let dir = fx.path().join(".git/fufu");
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(
        dir.join("autotrim.json"),
        r#"{"trimmed_at":0,"interval_secs":0}"#,
    )
    .expect("write due stamp");
}

/// Check whether `refs/fufu/trash/main` exists.
fn trash_exists(fx: &Fixture) -> bool {
    fx.try_git(&["rev-parse", "--verify", "refs/fufu/trash/main"])
        .status
        .success()
}

/// Extract stdout as a UTF-8 string.
fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

// ── Helper: build a fixture ready for a trim ──────────────────────────────

/// Commit, take two snapshots, set keep to `0s`, sleep so snapshots age past
/// the cutoff, and leave the stamp due. Returns the fixture.
fn due_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Take two snapshots so the chain has content to drop.
    fx.write("a.txt", "dirty1\n");
    let out = ff(&fx, &["-m", "one"]);
    assert!(
        out.status.success(),
        "snapshot one: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fx.write("a.txt", "dirty2\n");
    let out = ff(&fx, &["-m", "two"]);
    assert!(
        out.status.success(),
        "snapshot two: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Make snapshots old enough to drop: cutoff becomes "now".
    fx.set_config("fufu.keep", "0s");

    // Sleep so the existing snapshots are at least a second older than the
    // trim's clock, ensuring they fall outside the 0s keep window.
    std::thread::sleep(Duration::from_millis(1200));

    // Overwrite the stamp as due.
    write_due_stamp(&fx);

    fx
}

// ── Tests ─────────────────────────────────────────────────────────────────

/// First run stamps the clock and drops nothing (default 90-day keep).
#[test]
fn first_run_stamps_and_drops_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "bare ff succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Stamp was created
    let stamp = load_stamp(&fx);
    assert!(
        stamp["trimmed_at"].as_i64().unwrap() > 0,
        "trimmed_at > 0 after first run"
    );
    assert!(
        stamp["interval_secs"].as_i64().unwrap() == 0,
        "interval_secs == 0 (unset config)"
    );

    // No trash — default 90-day keep means nothing to drop.
    assert!(
        !trash_exists(&fx),
        "no trash ref on first run with default keep"
    );
}

/// A due stamp triggers an inline trim: snapshots are dropped, stdout is silent.
#[test]
fn a_due_stamp_runs_the_trim_inline() {
    let fx = due_fixture();

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "bare ff succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Trash ref now exists — trim ran.
    assert!(trash_exists(&fx), "trash ref exists after trim");

    // Stamp advanced.
    let stamp = load_stamp(&fx);
    assert!(
        stamp["trimmed_at"].as_i64().unwrap() > 0,
        "trimmed_at advanced past 0"
    );

    // The triggering command printed nothing about trimming.
    let out_text = stdout(&out);
    assert!(
        !out_text.to_lowercase().contains("trim") && !out_text.to_lowercase().contains("dropped"),
        "stdout must not mention trim or dropped:\n{out_text}"
    );
}

/// A fresh stamp defers the trim even when snapshots would be dropped.
#[test]
fn a_fresh_stamp_defers() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Take two snapshots.
    fx.write("a.txt", "dirty1\n");
    let out = ff(&fx, &["-m", "one"]);
    assert!(out.status.success(), "snapshot one");

    fx.write("a.txt", "dirty2\n");
    let out = ff(&fx, &["-m", "two"]);
    assert!(out.status.success(), "snapshot two");

    // Make snapshots old enough to drop.
    fx.set_config("fufu.keep", "0s");
    std::thread::sleep(Duration::from_millis(1200));

    // The stamp is still fresh from the last snapshot run — nothing is due.
    // Do NOT overwrite it.
    let out = ff(&fx, &[]);
    assert!(out.status.success(), "bare ff succeeded");

    // No trash — the cadence gate stopped the trim.
    assert!(
        !trash_exists(&fx),
        "no trash ref — fresh stamp deferred the trim"
    );
}

/// Setting `fufu.autoTrim false` disables the lane; the lane still stamps.
#[test]
fn autotrim_false_disables_the_lane() {
    let fx = due_fixture();
    fx.set_config("fufu.autoTrim", "false");

    let out = ff(&fx, &[]);
    assert!(out.status.success(), "bare ff succeeded");

    // No trash — lane is disabled.
    assert!(!trash_exists(&fx), "no trash when autoTrim is false");

    // Stamp shows disabled state with advanced trimmed_at.
    let stamp = load_stamp(&fx);
    assert!(
        stamp["interval_secs"].as_i64().unwrap() == -1,
        "interval_secs == -1 (disabled)"
    );
    assert!(
        stamp["trimmed_at"].as_i64().unwrap() > 0,
        "trimmed_at advanced (lane stamps so config is re-read daily)"
    );
}

/// CI=1 causes the lane to skip entirely — no stamp change, no trim.
#[test]
fn ci_skips_the_lane() {
    let fx = due_fixture();

    // Record the stamp bytes before the run.
    let stamp_path = fx.path().join(".git/fufu/autotrim.json");
    let stamp_before = std::fs::read(&stamp_path).expect("read stamp before");

    // Run with CI set.
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CI", "1")
        .output()
        .expect("spawn ff");
    assert!(out.status.success(), "bare ff with CI=1 succeeded");

    // No trash.
    assert!(!trash_exists(&fx), "no trash when CI is set");

    // Stamp is byte-identical — the lane returned before touching anything.
    let stamp_after = std::fs::read(&stamp_path).expect("read stamp after");
    assert_eq!(stamp_before, stamp_after, "stamp unchanged under CI");
}

/// The hook also carries the auto-trim lane.
#[test]
fn the_hook_carries_the_lane() {
    let fx = due_fixture();

    // Build a minimal hook payload (PreToolUse, like the hook tests use).
    let payload = format!(
        r#"{{"hook_event_name":"PreToolUse","session_id":"autotrim-test","cwd":{}}}"#,
        serde_json::to_string(&fx.path().display().to_string()).unwrap()
    );

    // Feed the payload to `ff hook agent trigger claude`.
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "agent", "trigger", "claude"])
        .current_dir(fx.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CI")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff hook");
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    let out = child.wait_with_output().expect("wait ff hook");
    assert!(
        out.status.success(),
        "ff hook succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Trash ref appears — the lane ran through the hook.
    assert!(
        trash_exists(&fx),
        "trash ref exists after hook with due lane"
    );
}

/// A manual `ff trim` resets the clock; a dry run does not.
#[test]
fn a_manual_trim_resets_the_clock() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Take a snapshot so there is something to trim.
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["-m", "snap"]);
    assert!(out.status.success(), "snapshot succeeded");

    // Write the stamp as due.
    write_due_stamp(&fx);

    // Run `ff trim` — a real trim.
    let out = ff(&fx, &["trim"]);
    assert!(
        out.status.success(),
        "ff trim succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stamp = load_stamp(&fx);
    assert!(
        stamp["trimmed_at"].as_i64().unwrap() > 0,
        "trimmed_at > 0 after manual trim"
    );

    // Write the stamp as due again.
    write_due_stamp(&fx);

    // Run `ff trim --dry-run` — a preview is not a run.
    let out = ff(&fx, &["trim", "--dry-run"]);
    assert!(out.status.success(), "ff trim --dry-run succeeded");

    let stamp = load_stamp(&fx);
    assert!(
        stamp["trimmed_at"].as_i64().unwrap() == 0,
        "trimmed_at still 0 after dry run"
    );
}

/// `ff config autoTrim` writes through to the cached cadence in the stamp.
#[test]
fn config_writes_through_to_the_cached_cadence() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Ensure the stamp file exists (bare ff creates it).
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &[]);
    assert!(out.status.success(), "bare ff succeeded");

    // `ff config autoTrim 2h` → stamp interval_secs == 7200
    let out = ff(&fx, &["config", "autoTrim", "2h"]);
    assert!(out.status.success(), "config autoTrim 2h succeeded");
    let stamp = load_stamp(&fx);
    assert!(
        stamp["interval_secs"].as_i64().unwrap() == 7200,
        "interval_secs == 7200 after setting 2h"
    );

    // `ff config autoTrim false` → -1
    let out = ff(&fx, &["config", "autoTrim", "false"]);
    assert!(out.status.success(), "config autoTrim false succeeded");
    let stamp = load_stamp(&fx);
    assert!(
        stamp["interval_secs"].as_i64().unwrap() == -1,
        "interval_secs == -1 after setting false"
    );

    // `ff config --unset autoTrim` → 0
    let out = ff(&fx, &["config", "--unset", "autoTrim"]);
    assert!(out.status.success(), "config --unset autoTrim succeeded");
    let stamp = load_stamp(&fx);
    assert!(
        stamp["interval_secs"].as_i64().unwrap() == 0,
        "interval_secs == 0 after unset"
    );
}
