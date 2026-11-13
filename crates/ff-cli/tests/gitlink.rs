//! Gitlinks through the real binary (#23): a change to one measures as git's
//! `Subproject commit` line on every diff surface, and a verb that moves the
//! working copy gives a gitlink an empty directory, as git's checkout does,
//! without touching what is inside it.

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
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
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

fn ok(out: Output) -> Output {
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    out
}

/// The paths `ff status --json` reports as changed in `dir`.
fn changes_at(dir: &Path) -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(&stdout(&ok(ff_at(dir, &["status", "--json"])))).expect("json");
    v["data"]["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .map(|c| c["path"].as_str().expect("path").to_string())
        .collect()
}

/// `main` with a gitlink `sub` pointing at the init commit, and `side` forked
/// from before the gitlink. The working copy has no `sub` directory, as in
/// the issue. Returns the fixture and the gitlink's target.
fn repo() -> (Fixture, String) {
    let fx = Fixture::new();
    fx.set_config("user.name", "Gitlink Tester");
    fx.set_config("user.email", "gitlink@test.test");
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    fx.git(&["branch", "side"]);
    fx.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{sha},sub"),
    ]);
    fx.git(&["commit", "-qm", "add sub"]);
    (fx, sha)
}

/// The issue's reproduction: a gitlink missing from the working copy.
#[test]
fn status_and_diff_measure_a_missing_gitlink() {
    let (fx, sha) = repo();
    let status = ok(ff(&fx, &["status"]));
    assert!(stdout(&status).contains("sub"), "{}", stdout(&status));
    let diff = stdout(&ok(ff(&fx, &["diff"])));
    assert!(diff.contains("deleted file mode 160000"), "{diff}");
    assert!(
        diff.contains(&format!("-Subproject commit {sha}")),
        "{diff}"
    );
}

/// Leaving and returning to a branch with a gitlink recreates its empty
/// directory, so the next snapshot does not record it as deleted.
#[test]
fn switch_round_trip_gives_a_gitlink_its_directory() {
    let (fx, _) = repo();
    std::fs::create_dir(fx.path().join("sub")).expect("mkdir sub");
    ok(ff(&fx, &["switch", "side"]));
    ok(ff(&fx, &["switch", "main"]));
    let sub = fx.path().join("sub");
    assert!(sub.is_dir(), "sub is a directory");
    assert_eq!(std::fs::read_dir(&sub).expect("read sub").count(), 0);
    assert_eq!(changes_at(&fx.path()), Vec::<String>::new());
}

/// Leaving a gitlink whose directory is empty, as git leaves it, removes it.
#[test]
fn switch_removes_an_empty_gitlink_directory() {
    let (fx, _) = repo();
    std::fs::create_dir(fx.path().join("sub")).expect("mkdir sub");
    ok(ff(&fx, &["switch", "side"]));
    assert!(!fx.path().join("sub").exists(), "sub is gone");
}

/// Leaving a gitlink whose directory holds a repository keeps its contents.
#[test]
fn switch_keeps_a_populated_gitlink_directory() {
    let (fx, _) = repo();
    let sub = fx.path().join("sub");
    fx.write("sub/inner.txt", "inner\n");
    fx.git_in(&sub, &["init", "-q"]);
    fx.git_in(&sub, &["add", "inner.txt"]);
    fx.git_in(&sub, &["commit", "-qm", "inner"]);
    ok(ff(&fx, &["switch", "side"]));
    assert!(
        sub.join("inner.txt").exists(),
        "the embedded repo is untouched"
    );
}

/// A worktree fufu makes gives a gitlink its directory, and is clean.
#[test]
fn worktree_gives_a_gitlink_its_directory() {
    let (fx, _) = repo();
    std::fs::create_dir(fx.path().join("sub")).expect("mkdir sub");
    let w = fx.root().join("w");
    ok(ff(&fx, &["worktree", w.to_str().unwrap()]));
    assert!(w.join("sub").is_dir(), "w/sub is a directory");
    assert_eq!(changes_at(&w), Vec::<String>::new());
}
