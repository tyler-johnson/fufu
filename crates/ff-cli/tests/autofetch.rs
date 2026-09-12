//! The fetch lane: drives the real `ff` binary against a file remote to prove
//! when the lane fetches, when it stays home, what it prunes, and that no
//! outcome of its fetch reaches the verb's exit.
//!
//! Every fixture pins `fufu.autoFetch false`, so each test here unsets it
//! first; the runner removes `CI` for the same reason autotrim's does. A
//! second clone, `mover`, is the teammate whose pushes the lane exists to
//! notice.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::fixtures::{Fixture, null_device};

// ── Runner ────────────────────────────────────────────────────────────────

/// A HOME no test may escape. `ff doctor --fix` and the wiring verbs write
/// into the user's own config files, so a suite that let the real HOME
/// through would rewrite the config of whoever is running it.
fn scratch_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::TempDir::new().expect("scratch HOME"))
        .path()
}

fn ff_env(fx: &Fixture, args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", scratch_home())
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
        .envs(env.iter().copied())
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_env(fx, args, &[])
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn ok(output: &Output) -> String {
    assert!(output.status.success(), "{}", out(output));
    stdout(output)
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).unwrap_or_else(|err| {
        panic!("not json ({err}): {}", out(output));
    })
}

// ── The stamp ─────────────────────────────────────────────────────────────

fn stamp_path(fx: &Fixture) -> std::path::PathBuf {
    fx.path().join(".git/fufu/autofetch.json")
}

/// The stamp as written, or the default shape when none has been.
fn load_stamp(fx: &Fixture) -> serde_json::Value {
    match std::fs::read_to_string(stamp_path(fx)) {
        Ok(data) => serde_json::from_str(&data).expect("stamp is json"),
        Err(_) => serde_json::json!({
            "fetched_at": 0, "interval_secs": 0, "remote": "",
            "last_error": null, "failed_since": 0
        }),
    }
}

/// A stamp older than any cadence: the next carrier fetches.
fn write_due_stamp(fx: &Fixture) {
    let dir = fx.path().join(".git/fufu");
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(
        dir.join("autofetch.json"),
        r#"{"fetched_at":0,"interval_secs":0}"#,
    )
    .expect("write due stamp");
}

/// A stamp taken a moment ago: nothing is due for the next ten minutes.
fn write_fresh_stamp(fx: &Fixture) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dir = fx.path().join(".git/fufu");
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(
        dir.join("autofetch.json"),
        format!(r#"{{"fetched_at":{now},"interval_secs":0,"remote":"origin"}}"#),
    )
    .expect("write fresh stamp");
}

// ── Fixture ───────────────────────────────────────────────────────────────

/// A clone with `main` pushed and tracking, the lane switched back on, and
/// a `mover` clone beside it for the teammate's pushes.
fn cloned_with_mover() -> (Fixture, std::path::PathBuf) {
    let fx = Fixture::new_cloned();
    fx.git(&["config", "--unset", "fufu.autoFetch"]);
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);

    let mover = fx.root().join("mover");
    fx.git_in(
        fx.root(),
        &["clone", "-q", &fx.remote_path().to_string_lossy(), "mover"],
    );
    fx.git_in(&mover, &["config", "user.name", "Mover"]);
    fx.git_in(&mover, &["config", "user.email", "mover@fetch.test"]);
    (fx, mover)
}

/// The teammate pushes one more commit to `main`; its sha comes back.
fn teammate_pushes(fx: &Fixture, mover: &Path, name: &str) -> String {
    std::fs::write(mover.join(format!("{name}.txt")), format!("{name}\n")).unwrap();
    fx.git_in(mover, &["add", "-A"]);
    fx.git_in(mover, &["commit", "-q", "-m", name]);
    fx.git_in(mover, &["push", "-q", "origin", "main"]);
    fx.git_in(mover, &["rev-parse", "HEAD"]).trim().to_string()
}

fn tracking_main(fx: &Fixture) -> String {
    fx.git(&["rev-parse", "refs/remotes/origin/main"])
        .trim()
        .to_string()
}

fn tracking_ref_exists(fx: &Fixture, name: &str) -> bool {
    fx.try_git(&["rev-parse", "--verify", "-q", name])
        .status
        .success()
}

// ── Tests ─────────────────────────────────────────────────────────────────

/// A stale stamp is a fetch: `ff status` reads the teammate's tip, its own
/// output is status's as before, and the stamp records the run. Stderr is
/// not a terminal here, so the lane says nothing, and `--json`'s envelope
/// is the same one.
#[test]
fn a_due_stamp_fetches_before_the_verb_runs() {
    let (fx, mover) = cloned_with_mover();
    let theirs = teammate_pushes(&fx, &mover, "theirs");
    assert_ne!(tracking_main(&fx), theirs, "not fetched yet");
    write_due_stamp(&fx);

    let output = ff(&fx, &["status"]);
    let text = ok(&output);
    assert_eq!(tracking_main(&fx), theirs, "the lane fetched");
    assert!(text.starts_with("on main · 1 to pull"), "{text}");
    assert_eq!(stderr(&output), "", "nothing said off a terminal");

    let stamp = load_stamp(&fx);
    assert!(stamp["fetched_at"].as_i64().unwrap() > 0, "{stamp}");
    assert_eq!(stamp["remote"], "origin", "{stamp}");
    assert!(stamp["last_error"].is_null(), "{stamp}");
    assert_eq!(stamp["failed_since"], 0, "{stamp}");

    // The same under --json: the envelope is status's, and stderr is empty.
    let theirs = teammate_pushes(&fx, &mover, "again");
    write_due_stamp(&fx);
    let output = ff(&fx, &["--json", "status"]);
    let v = json(&output);
    assert_eq!(v["ff"], 1, "{v}");
    assert_eq!(v["cmd"], "status", "{v}");
    assert_eq!(v["data"]["remote"], "origin", "{v}");
    assert_eq!(stderr(&output), "");
    assert_eq!(
        tracking_main(&fx),
        theirs,
        "the lane fetched under --json too"
    );
}

/// Inside the cadence the lane stays home: the teammate's next push is not
/// seen until the stamp is due again.
#[test]
fn inside_the_cadence_nothing_fetches() {
    let (fx, mover) = cloned_with_mover();
    write_due_stamp(&fx);
    ok(&ff(&fx, &["status"]));
    let before = tracking_main(&fx);

    let theirs = teammate_pushes(&fx, &mover, "later");
    ok(&ff(&fx, &["status"]));
    assert_eq!(tracking_main(&fx), before, "the stamp is fresh");
    assert_ne!(tracking_main(&fx), theirs);
}

/// `--fetch` fetches when nothing is due; `--no-fetch` does not when it is;
/// `autoFetch false` never does, and stamps the setting; `CI` set never does.
#[test]
fn the_flags_the_setting_and_ci_decide_the_lane() {
    let (fx, mover) = cloned_with_mover();

    let theirs = teammate_pushes(&fx, &mover, "one");
    write_fresh_stamp(&fx);
    ok(&ff(&fx, &["status"]));
    assert_ne!(tracking_main(&fx), theirs, "fresh: stays home");
    ok(&ff(&fx, &["status", "--fetch"]));
    assert_eq!(tracking_main(&fx), theirs, "--fetch: fetches anyway");

    let theirs = teammate_pushes(&fx, &mover, "two");
    write_due_stamp(&fx);
    ok(&ff(&fx, &["status", "--no-fetch"]));
    assert_ne!(
        tracking_main(&fx),
        theirs,
        "--no-fetch: stays home when due"
    );
    assert_eq!(load_stamp(&fx)["fetched_at"], 0, "and does not stamp");

    fx.set_config("fufu.autoFetch", "false");
    ok(&ff(&fx, &["status"]));
    assert_ne!(tracking_main(&fx), theirs, "autoFetch false: never");
    let stamp = load_stamp(&fx);
    assert_eq!(stamp["interval_secs"], -1, "{stamp}");
    assert!(
        stamp["fetched_at"].as_i64().unwrap() > 0,
        "off is stamped: {stamp}"
    );
    fx.git(&["config", "--unset", "fufu.autoFetch"]);

    write_due_stamp(&fx);
    ok(&ff_env(&fx, &["status"], &[("CI", "1")]));
    assert_ne!(tracking_main(&fx), theirs, "CI: never");
    assert_eq!(load_stamp(&fx)["fetched_at"], 0, "and does not stamp");

    // `--fetch` is the person asking, and is honored under CI too.
    ok(&ff_env(&fx, &["status", "--fetch"], &[("CI", "1")]));
    assert_eq!(tracking_main(&fx), theirs, "CI with --fetch: fetches");
}

/// A copy the remote no longer has loses its tracking ref on the next due
/// fetch: `ff branch` stops listing it, and a local branch tracking it
/// reads gone.
#[test]
fn a_deleted_copy_is_pruned_on_the_next_fetch() {
    let (fx, mover) = cloned_with_mover();
    fx.git(&["branch", "side"]);
    fx.git(&["push", "-q", "-u", "origin", "side"]);
    assert!(tracking_ref_exists(&fx, "refs/remotes/origin/side"));
    // A clone of a remote that held something has this too; the fixture's
    // remote was empty when cloned, so it is written the way clone would.
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);

    fx.git_in(&mover, &["push", "-q", "origin", ":side"]);
    write_due_stamp(&fx);
    ok(&ff(&fx, &["status"]));
    assert!(
        !tracking_ref_exists(&fx, "refs/remotes/origin/side"),
        "the tracking ref was pruned"
    );
    assert!(
        tracking_ref_exists(&fx, "refs/remotes/origin/main"),
        "the copy that is there stays"
    );
    assert!(
        tracking_ref_exists(&fx, "refs/remotes/origin/HEAD"),
        "the symbolic ref is not a copy and is left alone"
    );

    let v = json(&ff(&fx, &["--json", "branch"]));
    let named = v["data"]["named"].as_array().expect("named");
    let side = named
        .iter()
        .find(|row| row["name"] == "side")
        .expect("side is still a local branch");
    assert_eq!(side["upstream"]["gone"], true, "{v}");
    assert!(
        v["data"]["remote_only"].as_array().unwrap().is_empty(),
        "no remote-only row for a copy that is gone: {v}"
    );

    fx.git(&["switch", "-q", "side"]);
    write_fresh_stamp(&fx);
    let text = ok(&ff(&fx, &["status"]));
    assert!(text.starts_with("on side · remote is gone"), "{text}");
}

/// An unreachable remote costs the verb nothing but the stamp's record: the
/// exit is 0, stdout is status's, the stamp holds the failure, `ff doctor`
/// reports it, and the next success clears it.
#[test]
fn an_unreachable_remote_is_recorded_and_the_verb_still_answers() {
    let (fx, mover) = cloned_with_mover();
    let good = fx.remote_path().to_string_lossy().into_owned();
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");

    write_due_stamp(&fx);
    let output = ff(&fx, &["status"]);
    let text = ok(&output);
    assert!(text.starts_with("on main · "), "{text}");
    assert_eq!(stderr(&output), "");
    let stamp = load_stamp(&fx);
    assert!(
        stamp["last_error"]
            .as_str()
            .unwrap()
            .starts_with("fetching from origin failed"),
        "{stamp}"
    );
    assert!(stamp["failed_since"].as_i64().unwrap() > 0, "{stamp}");
    assert!(stamp["fetched_at"].as_i64().unwrap() > 0, "{stamp}");

    // Doctor's own run fetches too, and fails the same way; its row says
    // since when.
    // Doctor exits 1 on any finding, and this is one.
    let output = ff(&fx, &["doctor"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("WARN  auto-fetch     failing since"),
        "{text}"
    );
    assert!(text.contains("fetching from origin failed"), "{text}");

    // The remote comes back, and a success clears the record.
    fx.set_config("remote.origin.url", &good);
    let theirs = teammate_pushes(&fx, &mover, "back");
    write_due_stamp(&fx);
    ok(&ff(&fx, &["status"]));
    assert_eq!(tracking_main(&fx), theirs);
    let stamp = load_stamp(&fx);
    assert!(stamp["last_error"].is_null(), "{stamp}");
    assert_eq!(stamp["failed_since"], 0, "{stamp}");
    let text = out(&ff(&fx, &["doctor"]));
    assert!(text.contains("info  auto-fetch     last fetched"), "{text}");
    assert!(text.contains("from origin (at most every 10m)"), "{text}");
}

/// `ff pull` is a fetch, so it stamps the lane's clock — with the lane off
/// as much as on.
#[test]
fn pull_stamps_the_cadence() {
    let (fx, _mover) = cloned_with_mover();
    fx.set_config("fufu.autoFetch", "false");
    write_due_stamp(&fx);
    ok(&ff(&fx, &["pull"]));
    let stamp = load_stamp(&fx);
    assert!(stamp["fetched_at"].as_i64().unwrap() > 0, "{stamp}");
    assert_eq!(stamp["remote"], "origin", "{stamp}");
    assert!(stamp["last_error"].is_null(), "{stamp}");
}

/// `--fetch` on a verb with no fetch lane is refused by name; on pull, the
/// verb that fetches, it is accepted as what pull does anyway. The pair
/// together is refused on either side of the verb.
#[test]
fn fetch_on_a_verb_without_the_lane_is_not_here() {
    let (fx, _mover) = cloned_with_mover();
    let output = ff(&fx, &["undo", "--fetch"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert!(
        stderr(&output).contains(
            "ff: undo reads nothing from the remote, so there is nothing for --fetch to refresh"
        ),
        "{}",
        out(&output)
    );
    let v = json(&ff(&fx, &["--json", "op", "log", "--fetch"]));
    assert_eq!(v["error"]["id"], "fetch/not-here", "{v}");

    ok(&ff(&fx, &["pull", "--fetch"]));
    ok(&ff(&fx, &["pull", "--no-fetch"]));
    ok(&ff(&fx, &["--no-fetch", "pull"]));

    let output = ff(&fx, &["--fetch", "status", "--no-fetch"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v = json(&ff(&fx, &["--json", "--fetch", "status", "--no-fetch"]));
    assert_eq!(v["error"]["id"], "usage/bad-flags", "{v}");
}

/// `ff config autoFetch <cadence>` writes the encoding through to the stamp,
/// the way autoTrim's does, so the hot path reads the new cadence at once.
#[test]
fn config_writes_the_cadence_through_to_the_stamp() {
    let (fx, _mover) = cloned_with_mover();
    write_fresh_stamp(&fx);
    ok(&ff(&fx, &["config", "autoFetch", "2h"]));
    assert_eq!(load_stamp(&fx)["interval_secs"], 7200);
    ok(&ff(&fx, &["config", "autoFetch", "false"]));
    assert_eq!(load_stamp(&fx)["interval_secs"], -1);
    ok(&ff(&fx, &["config", "autoFetch", "--unset"]));
    assert_eq!(load_stamp(&fx)["interval_secs"], 0);
}

/// Doctor fetches every run — the remote floor is what it checks — and
/// not with the setting off.
#[test]
fn doctor_fetches_every_run_unless_the_lane_is_off() {
    let (fx, mover) = cloned_with_mover();
    let theirs = teammate_pushes(&fx, &mover, "one");
    write_fresh_stamp(&fx);
    // Doctor's exit is its findings', and an unwired fixture has some.
    out(&ff(&fx, &["doctor"]));
    assert_eq!(
        tracking_main(&fx),
        theirs,
        "doctor fetched on a fresh stamp"
    );

    let theirs = teammate_pushes(&fx, &mover, "two");
    fx.set_config("fufu.autoFetch", "false");
    let text = out(&ff(&fx, &["doctor"]));
    assert_ne!(tracking_main(&fx), theirs, "off is off for doctor too");
    assert!(
        text.contains("info  auto-fetch     off (the autoFetch setting)"),
        "{text}"
    );
}

/// The lane fetches branches and nothing else: a tag on the remote stays
/// there, so no verb reports a tag write as motion made outside fufu. Pull
/// fetches tags as it always has.
#[test]
fn the_lane_leaves_tags_to_pull() {
    let (fx, mover) = cloned_with_mover();
    fx.git_in(&mover, &["tag", "v1"]);
    fx.git_in(&mover, &["push", "-q", "origin", "v1"]);

    write_due_stamp(&fx);
    ok(&ff(&fx, &["status"]));
    assert!(
        !tracking_ref_exists(&fx, "refs/tags/v1"),
        "no tag from the lane"
    );
    let text = ok(&ff(&fx, &["op", "log"]));
    assert!(!text.contains("foreign"), "no absorb row: {text}");

    ok(&ff(&fx, &["pull"]));
    assert!(
        tracking_ref_exists(&fx, "refs/tags/v1"),
        "pull brings the tag"
    );
}
