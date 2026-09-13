//! `ff explain` and the exits a failure prints: the registry is one source
//! for both, so a raise site that says nothing borrows the id's ways out,
//! and an id with none names its own explanation.

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
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// The exits a raise site never wrote down. Most `Error::coded` calls pass
/// none — the way out belongs to the id, and the registry has held it all
/// along — so the failure reads what `ff explain` reads. Before this, `no
/// branch named x` was a dead end, and the next thing tried was git.
#[test]
fn a_failure_with_no_exits_of_its_own_borrows_the_registrys() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out = ff(&fx, &["switch", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no branch named nope"), "{stderr}");
    assert!(stderr.contains("try:"), "{stderr}");
    assert!(stderr.contains("ff branch"), "{stderr}");
}

/// Both surfaces read the one registry, so a machine is told what a terminal
/// would be.
#[test]
fn the_borrowed_exits_reach_the_json_envelope_too() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out = ff(&fx, &["switch", "nope", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "branch/not-found");
    let exits = v["error"]["exits"].as_array().expect("exits array");
    assert!(
        exits.iter().any(|e| e == "ff branch"),
        "envelope carries the registry's exits: {exits:?}"
    );
}

/// The floor under both: an id whose registry entry has no exits either is
/// still not a dead end, because the entry itself is the thing to read.
#[test]
fn an_id_with_no_exits_anywhere_names_its_own_explanation() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let bare = dir.path().join("bare.git");
    std::process::Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&bare)
        .output()
        .expect("git init --bare");
    let out = ff_at(&bare, &["status"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ff explain repo/bare"),
        "a coded failure always leaves somewhere to go: {stderr}"
    );
}

#[test]
fn human_error_lists_its_exits() {
    let fx = Fixture::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["describe"])
        .env("FF_NONINTERACTIVE", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("try:"), "stderr contains try:");
    assert!(
        stderr.contains("ff describe -m <msg>"),
        "stderr contains hint"
    );
}

#[test]
fn uncoded_errors_report_as_internal_and_exit_one() {
    // Detached HEAD causes ff describe to return Error::coded("repo/detached", ...).
    // -m bypasses the non-interactive guard so the detached-HEAD path is reached.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["checkout", "--detach", "HEAD"]);
    let out = ff(&fx, &["describe", "-m", "test", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["error"]["id"], "repo/detached");
}

/// `ff explain <id>` works outside a repository and renders the entry.
#[test]
fn explain_known_id_works_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "repo/bare"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("repo/bare"));
    assert!(text.contains("bare repository"));
}

/// `ff explain --list` renders all entries, works outside a repository.
#[test]
fn explain_list_works_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "--list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("repo/bare"));
    assert!(text.contains("branch/not-found"));
    assert!(text.contains("internal"));
}

/// `ff explain <id> --json` emits the versioned envelope with entry data.
#[test]
fn explain_json_single_entry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "branch/exists", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1, "envelope version");
    assert_eq!(v["cmd"], "explain");
    assert_eq!(v["data"]["id"], "branch/exists");
    assert!(v["data"]["summary"].is_string());
    assert!(v["data"]["detail"].is_string());
    assert!(v["data"]["exits"].is_array());
    assert!(
        v["data"]["exit"].is_number(),
        "each entry carries its exit code"
    );
}

/// `exit` on an explain entry is the code the id exits with: 2 for the
/// `usage/` namespace, 4 for `ref/contended`, the one id coded on its own.
#[test]
fn explain_json_carries_the_exit_code() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "ref/contended", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["data"]["exit"], 4);

    let out = ff_at(tmp.path(), &["explain", "usage/bad-flags", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["data"]["exit"], 2);
}

/// `ff explain --list --json` emits an array of entries.
#[test]
fn explain_json_list() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "--list", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "explain");
    let entries = v["data"]["entries"].as_array().expect("entries is array");
    assert!(entries.len() >= 16, "registry has entries");
    for entry in entries {
        assert!(entry["id"].is_string(), "each entry has id");
        assert!(entry["summary"].is_string(), "each entry has summary");
        assert!(entry["exit"].is_number(), "each entry has an exit code");
    }
}

/// `ff explain <unknown-id>` exits 2 (usage/) with usage/unknown-error-id.
#[test]
fn explain_unknown_id_exits_two() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "nonexistent/foo", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/unknown-error-id");
}

/// `ff explain` with no arguments exits 2 (usage error).
#[test]
fn explain_no_args_exits_two() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain"]);
    assert_eq!(out.status.code(), Some(2));
}

/// Human explain output includes try: hints when the entry has exits.
#[test]
fn explain_human_includes_hints() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "branch/not-found"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("try:"));
    assert!(text.contains("ff branch"));
}

/// Human explain output omits try: block when the entry has no exits.
#[test]
fn explain_human_no_hints_when_empty() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "repo/bare"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("try:"), "no exits means no try: block");
}
