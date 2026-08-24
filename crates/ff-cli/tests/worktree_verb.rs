//! `ff worktree list` — the verb that reads the survey, run through the real
//! `ff` binary.
//!
//! The linked-worktree model lives in `tests/worktree.rs`; this file is the
//! reader: the bare form, the envelope name, the orphan section, and the
//! exit that has been waiting for this verb to exist.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
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

fn ok_at(dir: &Path, args: &[&str]) -> String {
    let output = ff_at(dir, args);
    assert!(
        output.status.success(),
        "ff {args:?} failed: {}",
        out(&output)
    );
    stdout(&output)
}

fn ok(fx: &Fixture, args: &[&str]) -> String {
    ok_at(&fx.path(), args)
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Worktree Tester");
    fx.set_config("user.email", "worktree@test.test");
    fx
}

/// The linked worktree beside the repository, on a branch of its own.
fn bay(fx: &Fixture) -> PathBuf {
    let bay = fx.root().join("bay");
    fx.git(&["worktree", "add", "-q", "-b", "side", bay.to_str().unwrap()]);
    bay
}

/// A bay with work in it, then removed: the setup every orphan test wants.
fn gone_bay(fx: &Fixture) -> PathBuf {
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(fx);
    // A status inside the bay gives its chain a floor, so the orphan row
    // has a tip to name.
    ok_at(&bay, &["status"]);
    fx.git(&["worktree", "remove", "--force", bay.to_str().unwrap()]);
    bay
}

/// The bare form and the spelled-out one emit the same envelope: the cmd is
/// the full path, never the family — the rule the `ff op` family exists to
/// enforce.
#[test]
fn bare_worktree_is_the_list() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let a = json(&ff(&fx, &["worktree", "--json"]));
    let b = json(&ff(&fx, &["worktree", "list", "--json"]));
    assert_eq!(a["cmd"], "worktree list");
    assert_eq!(b["cmd"], "worktree list");
    assert_eq!(a["data"], b["data"]);
}

/// A repository with no bays still lists its one worktree, and marks it
/// current.
#[test]
fn a_lone_repository_lists_one_row() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("main"), "names main: {body}");
    assert!(body.contains("* main"), "marks it current: {body}");
}

/// A bay is a row of its own: its id, and the branch it stands on.
#[test]
fn a_bay_is_listed_with_its_branch() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("bay"), "names the bay: {body}");
    assert!(body.contains("side"), "names its branch: {body}");

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let worktrees = v["data"]["worktrees"].as_array().expect("worktrees array");
    assert_eq!(worktrees.len(), 2, "two entries: {v}");
}

/// A bay's chain outlives its checkout: after `git worktree remove` it is
/// the orphan section, and its tip survives for the reader to take.
#[test]
fn a_removed_bay_becomes_an_orphan_row() {
    let fx = repo();
    gone_bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(
        body.contains("chains whose worktree is gone"),
        "the section header: {body}"
    );
    assert!(body.contains("bay"), "names the gone bay: {body}");

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let orphans = v["data"]["orphans"].as_array().expect("orphans array");
    assert_eq!(orphans.len(), 1, "one orphan: {v}");
    assert!(
        orphans[0]["tip"].is_string(),
        "tip is not null: {}",
        orphans[0]
    );
}

/// The orphan row says how to use its tip: it names `ff restore` and
/// `--at-op`. The row is the front door for a deleted bay, and the test
/// that it says so is the point.
#[test]
fn the_orphan_row_names_the_way_back() {
    let fx = repo();
    gone_bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("ff restore"), "names ff restore: {body}");
    assert!(body.contains("--at-op"), "names --at-op: {body}");
}

/// The `branch/checked-out-elsewhere` exit has pointed at `git worktree
/// list` since fufu had no worktree verb; it names fufu's own verb now.
#[test]
fn the_exit_now_names_fufus_own_verb() {
    let fx = repo();

    let body = ok(&fx, &["explain", "branch/checked-out-elsewhere"]);
    assert!(
        body.contains("ff worktree list"),
        "names ff worktree list: {body}"
    );
}
