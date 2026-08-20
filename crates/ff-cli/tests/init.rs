//! `ff init` end to end: the three shapes it has to keep apart, and the two
//! things it leaves behind that `git init` does not.
//!
//! The shapes are a fresh directory, the same directory a second time, and a
//! directory `git init` already made. One of those creates a repository and
//! two of them adopt one, and reporting all three the same way would be the
//! verb quietly lying about which it did.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;

/// `ff` with a hermetic environment: no global or system config, and an
/// identity from the environment rather than from a config file that does
/// not exist. Every commit fufu writes — including the floor — needs one.
fn ff(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Init Author")
        .env("GIT_AUTHOR_EMAIL", "author@init.test")
        .env("GIT_COMMITTER_NAME", "Init Committer")
        .env("GIT_COMMITTER_EMAIL", "committer@init.test")
        .env_remove("FF_SESSION")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let out = ff(dir, args);
    assert!(out.status.success(), "ff {args:?} failed: {}", stderr(&out));
    stdout(&out)
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn git")
}

fn config(dir: &Path, key: &str) -> String {
    let out = git(dir, &["config", "--get", key]);
    String::from_utf8(out.stdout).expect("utf-8").trim().into()
}

/// The op log's rows, captures included — the floor is a note and the default
/// view leaves notes' machine-rate company out.
fn op_log(dir: &Path) -> Vec<String> {
    ok(dir, &["op", "log", "--captures"])
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn a_fresh_init_lands_on_main_with_the_guard_and_a_floor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();

    let body = ok(dir, &["init"]);
    assert!(
        body.contains("initialized an empty repository on main"),
        "{body}"
    );
    assert!(
        body.contains("the net is on"),
        "the tail is the earn: {body}"
    );

    // The gc guard, before anything could have expired.
    assert_eq!(config(dir, "gc.refs/fufu/*.reflogExpire"), "never");
    assert_eq!(
        config(dir, "gc.refs/fufu/*.reflogExpireUnreachable"),
        "never"
    );

    // The floor: one operation, and it is the log's root note.
    let rows = op_log(dir);
    assert_eq!(rows.len(), 1, "exactly the floor: {rows:#?}");
    assert!(rows[0].contains("note"), "the floor is a note: {rows:#?}");
}

/// A second `ff init` is the adopt case, and says so. It also takes no second
/// floor — the log it found is the log it keeps.
#[test]
fn a_second_init_adopts_and_adds_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();
    ok(dir, &["init"]);
    let before = op_log(dir);

    let body = ok(dir, &["init"]);
    assert!(body.contains("already a git repository on main"), "{body}");
    assert!(body.contains("the net is on"), "{body}");
    assert_eq!(op_log(dir), before, "no second floor");
}

/// The other adopt: a repository git made, which is what somebody who
/// installed fufu after starting work actually has.
#[test]
fn init_adopts_a_repository_git_made() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();
    assert!(git(dir, &["init", "-q", "-b", "main"]).status.success());
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();

    let body = ok(dir, &["init"]);
    assert!(body.contains("already a git repository on main"), "{body}");
    assert_eq!(config(dir, "gc.refs/fufu/*.reflogExpire"), "never");

    // Two rows here, not one: the floor, and a capture of the work that was
    // already on disk before fufu had ever looked.
    let rows = op_log(dir);
    assert_eq!(rows.len(), 2, "floor plus the adopted worktree: {rows:#?}");
    assert!(rows[0].contains("capture"), "{rows:#?}");
    assert!(rows[1].contains("note"), "{rows:#?}");
}

/// `ff init <dir>` creates the directory as well as the repository.
#[test]
fn init_takes_a_directory() {
    let tmp = tempfile::TempDir::new().unwrap();
    let body = ok(tmp.path(), &["init", "made-here"]);
    assert!(body.contains("initialized an empty repository"), "{body}");
    assert!(tmp.path().join("made-here/.git").is_dir());
}

/// A bare repository has no working tree, so there is nothing for the floor
/// to hold — refused, and answered with the spelling that does work.
#[test]
fn bare_is_refused_and_names_the_git_form() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = ff(tmp.path(), &["init", "--bare"]);
    assert!(!out.status.success(), "--bare is refused");
    let err = stderr(&out);
    assert!(err.contains("no working tree"), "{err}");
    assert!(err.contains("ff git init --bare"), "the way out: {err}");
    assert!(
        !tmp.path().join(".git").exists() && !tmp.path().join("HEAD").exists(),
        "the refusal wrote nothing"
    );
}

/// The other bare shape: standing *inside* a bare repository git made. There
/// is still no working tree, so there is still nothing to arm — refused with
/// the id that names the state rather than the flag.
#[test]
fn init_inside_a_bare_repository_is_refused() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();
    assert!(
        git(dir, &["init", "-q", "--bare", "-b", "main"])
            .status
            .success()
    );

    let out = ff(dir, &["init", "--json"]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json envelope");
    assert_eq!(v["error"]["id"], "repo/bare");
}

/// The machine envelope carries the model the prose reports from.
#[test]
fn the_json_envelope_carries_path_branch_created_and_floor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();

    let body = ok(dir, &["init", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "init");
    assert_eq!(v["data"]["branch"], "main");
    assert_eq!(v["data"]["created"], true);
    // Resolved, not echoed: `.` would say nothing a caller did not know.
    let path = v["data"]["path"].as_str().expect("a path");
    assert!(Path::new(path).is_absolute(), "{path}");
    // The floor is addressed in letters, never hex.
    let floor = v["data"]["floor"].as_str().expect("a floor");
    assert!(
        floor.chars().all(|c| ('k'..='z').contains(&c)),
        "letters, not hex: {floor}"
    );

    // And the adopt run says `created: false` against the same repository.
    let body = ok(dir, &["init", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(v["data"]["created"], false);
    assert_eq!(v["data"]["floor"].as_str(), Some(floor));
}

/// The error a person gets for being outside a repository now has somewhere
/// to send them, which is the whole reason these two verbs exist.
#[test]
fn being_outside_a_repository_names_init_and_clone() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = ff(tmp.path(), &["status"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("ff init"), "{err}");
    assert!(err.contains("ff clone"), "{err}");
}
