//! `ff status` pins the standing work on the branch underfoot: a held
//! rewrite, and the resolution open on it. Status reports — it never adopts
//! the verb's exit code, and a lookup that cannot run is a missing line,
//! never a failed status.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
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

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

/// Both streams concatenated, so an assertion never misses the one an
/// output actually landed on.
fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Status Tester");
    fx.set_config("user.email", "status@test.test");
    fx
}

/// `feature` and `main` edit the same line of the same file from the same
/// base, so a restack of `feature` conflicts and holds. Leaves the fixture
/// standing on `feature`.
fn held_stack(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    // The conflicting restack holds — the precondition for a standing hold.
    let held = ff(fx, &["restack"]);
    assert_eq!(held.status.code(), Some(3), "{}", out(&held));
}

/// A held rewrite is standing work: `ff status` names it and the way out,
/// and exits 0 — status reports, it does not adopt the verb's exit code.
#[test]
fn status_pins_a_standing_hold() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("held:"), "the hold is named: {text}");
    assert!(
        text.contains("ff restack"),
        "and so is the verb that recorded it: {text}"
    );
    assert!(
        text.contains("in 1 file"),
        "the count of the conflict's files: {text}"
    );
    assert!(
        text.contains("ff resolve to fix them"),
        "and the way out: {text}"
    );
}

/// An open resolution is the more urgent fact: it says the conflicts are in
/// the working tree, names `ff done`, and stands above the hold.
#[test]
fn status_pins_an_open_resolution() {
    let fx = repo();
    held_stack(&fx);
    assert!(ff(&fx, &["resolve"]).status.success());

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("resolving:"),
        "the resolution is named: {text}"
    );
    assert!(
        text.contains("in your working tree"),
        "the markers' location is said: {text}"
    );
    assert!(text.contains("ff done"), "and the way out: {text}");
    assert!(
        text.contains("ff resolve --abandon to drop it"),
        "and the way out of the session: {text}"
    );
    let resolving = text.find("resolving:").expect("the resolution block");
    let held = text.find("held:").expect("the hold block");
    assert!(
        resolving < held,
        "the resolution goes above the hold: {text}"
    );
}

/// The JSON envelope carries the hold under its key while the resolution is
/// null, and the resolution under its key once `ff resolve` opens it — the
/// hold stays, because it is what the session is resolving.
#[test]
fn status_json_carries_both() {
    let fx = repo();
    held_stack(&fx);

    let v = json(&ff(&fx, &["status", "--json"]));
    let held = &v["data"]["held"];
    assert!(!held.is_null(), "the hold is under its key: {held}");
    assert_eq!(held["verb"], "restack");
    assert!(
        held["paths"]
            .as_array()
            .unwrap()
            .contains(&serde_json::Value::String("f.txt".into())),
        "the conflict's file: {held}"
    );
    assert_eq!(held["at"]["what"], "commit");
    assert!(
        v["data"]["resolving"].is_null(),
        "no session is open yet: {:?}",
        v["data"]["resolving"]
    );

    assert!(ff(&fx, &["resolve"]).status.success());

    let v = json(&ff(&fx, &["status", "--json"]));
    let resolving = &v["data"]["resolving"];
    assert!(
        !resolving.is_null(),
        "the resolution is under its key: {resolving}"
    );
    assert_eq!(resolving["verb"], "restack");
    assert_eq!(resolving["conflicts"], 1);
    assert!(
        !resolving["steps"].as_array().unwrap().is_empty(),
        "what the session will land: {resolving}"
    );
    assert!(
        !v["data"]["held"].is_null(),
        "the hold stays: it is what the session is resolving"
    );
}

/// Corrupt the branch's metadata by hand and the hold is a missing line,
/// never a failed `ff status`: exit 0, and the rest of the status still
/// renders.
#[test]
fn status_survives_an_unreadable_hold() {
    let fx = repo();
    held_stack(&fx);

    let meta = fx.path().join(".git/fufu/branch/feature");
    std::fs::write(&meta, "not json at all").unwrap();

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "a missing line, never a failed status: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.starts_with("on feature"),
        "the rest of the status still renders: {text:?}"
    );
    assert!(
        text.contains("no changes"),
        "the open change row is still there: {text:?}"
    );
    assert!(
        !text.contains("held:"),
        "the unreadable hold is a missing line: {text:?}"
    );
}

/// The third of the three held-rewrite disciplines is exits blocked: sync
/// refuses to publish while a hold stands, and a guard nobody is told about
/// is a guard that surprises people — so the status says so.
#[test]
fn a_standing_hold_says_the_exit_is_blocked() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("exits are blocked"),
        "the exit is named as blocked: {text}"
    );
    assert!(
        text.contains("ff sync will not publish"),
        "and the verb it will hold back: {text}"
    );
    let held = text.find("held:").expect("the hold block");
    let blocked = text.find("exits are blocked").expect("the blocked note");
    assert!(held < blocked, "the blocked note follows the hold: {text}");
}

/// `ff status` is `ff log` cropped to two rows, and a crop must not lose a
/// column. The parent row's first field is the commit's chain-segment anchor
/// — the capture it was cut from — and both views have to name the same one.
#[test]
fn the_parent_row_names_the_same_segment_ff_log_does() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let commit = ff(&fx, &["commit", "-m", "one"]);
    assert!(commit.status.success(), "{}", out(&commit));

    let parent = json(&ff(&fx, &["status", "--json"]))["data"]["parent"].clone();
    let segment = parent["segment"].as_str().unwrap_or_default().to_string();
    assert_eq!(
        segment.len(),
        40,
        "the parent row carries its anchor, as hex: {segment:?}"
    );

    // Rather than recompute the letters spelling here, read the column off
    // both views: what matters is that the crop and the full page name the
    // same capture, not how either of them spells it.
    let commit_column = |text: &str| -> String {
        text.lines()
            .find(|line| line.starts_with('●'))
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("<no commit row>")
            .to_string()
    };
    let from_status = commit_column(&stdout(&ff(&fx, &["status"])));
    let from_log = commit_column(&stdout(&ff(&fx, &["log", "-n", "2"])));
    assert_ne!(
        from_status, "—",
        "the parent row printed the empty-column dash over a real anchor"
    );
    assert_eq!(
        from_status, from_log,
        "status and log disagree about the parent commit's segment"
    );
}

/// Two remotes, neither named `origin`, and a branch whose own section names
/// none of them: `ff status` says the remote cannot be named, and does not
/// let the empty remote axis read as settled. It still exits 0 — status
/// reports, it never adopts a verb's exit code.
#[test]
fn status_says_the_remote_cannot_be_named() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);

    // Two remotes, neither `origin` — the shape that leaves `for_branch`
    // with nothing to name.
    fx.set_config("remote.one.url", "/nonexistent/one.git");
    fx.set_config("remote.one.fetch", "+refs/heads/*:refs/remotes/one/*");
    fx.set_config("remote.two.url", "/nonexistent/two.git");
    fx.set_config("remote.two.fetch", "+refs/heads/*:refs/remotes/two/*");

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("remote unnamed"),
        "the unnameable remote is said: {text}"
    );
    assert!(
        !text.contains("nothing to sync"),
        "an empty axis never reads as settled: {text}"
    );
}

/// One remote named `origin` that the branch's own section points at is a
/// named, settled remote: the `remote unnamed` part never appears.
#[test]
fn a_settled_remote_still_says_nothing_to_sync() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);

    fx.set_config("remote.origin.url", "/nonexistent/origin.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        !text.contains("remote unnamed"),
        "a named remote is not unnameable: {text}"
    );
}
