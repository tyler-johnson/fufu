//! One chain per worktree, one branch per worktree: the linked-worktree half
//! of the model, run through the real `ff` binary.
//!
//! No other CLI test has ever stood `ff` inside a linked worktree, which is
//! how the three regressions it guards against could each pass review. Every
//! test here fails on the build from before the landing; together they are
//! that reproduction, kept.

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

/// The reproduction of the corruption: an undo in the main worktree must not
/// move the bay's branch, the bay's tree, or main's own.
#[test]
fn undo_in_main_leaves_the_bay_alone() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);

    // The bay does its own work on its own branch.
    std::fs::write(bay.join("bay.txt"), "bay work\n").expect("write bay file");
    ok_at(&bay, &["commit", "-m", "bay work"]);
    let side = fx.git(&["rev-parse", "refs/heads/side"]).trim().to_string();
    assert_eq!(
        fx.git_in(&bay, &["rev-parse", "HEAD"]).trim(),
        side,
        "the bay stands on side"
    );

    // Main does its own, so its chain has a step to give back.
    fx.write("main.txt", "main work\n");
    ok(&fx, &["commit", "-m", "main work"]);

    ok(&fx, &["undo"]);

    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main",
        "main's HEAD is still on main"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/heads/side"]).trim(),
        side,
        "the undo did not move another worktree's branch"
    );
    assert_eq!(
        std::fs::read_to_string(bay.join("bay.txt")).expect("read bay file"),
        "bay work\n",
        "the bay's working tree still holds what the bay wrote"
    );
    assert!(
        !fx.path().join("bay.txt").exists(),
        "the bay's file must not appear in main's worktree"
    );
}

/// A read in the bay touches the bay's chain only: main's tip is invariant,
/// the bay bootstraps its own, and each `op log` names only its own work.
#[test]
fn each_worktree_keeps_its_own_chain() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);

    fx.write("main.txt", "main work\n");
    ok(&fx, &["commit", "-m", "main work"]);
    let main_tip = fx
        .git(&["rev-parse", "refs/fufu/wt/main/ops"])
        .trim()
        .to_string();

    // A read in the bay. On the shared-chain build this appended to main's
    // log just by being read.
    ok_at(&bay, &["status"]);

    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/wt/main/ops"]).trim(),
        main_tip,
        "a read in the bay left main's chain tip where it was"
    );

    // The bay's chain is keyed by the worktree's id, and it holds its own tip.
    let bay_tip = fx
        .git(&["rev-parse", "refs/fufu/wt/bay/ops"])
        .trim()
        .to_string();
    assert_ne!(bay_tip, main_tip, "the bay's chain tip is its own");

    let main_log = ok(&fx, &["op", "log", "-n", "0"]);
    let bay_log = ok_at(&bay, &["op", "log", "-n", "0"]);
    assert!(
        main_log.contains("main work"),
        "main's log names main's commit: {main_log}"
    );
    assert!(
        !bay_log.contains("main work"),
        "the bay's log holds only the bay's work: {bay_log}"
    );
}

/// Both directions of the exclusivity guard, and the refusal changes nothing.
#[test]
fn a_branch_is_only_ever_open_in_one_worktree() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);

    // Main cannot take the bay's branch.
    let output = ff(&fx, &["switch", "side"]);
    assert!(
        !output.status.success(),
        "main took the bay's branch: {}",
        out(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("'side' is already used by worktree at"),
        "{err}"
    );
    assert!(
        err.contains(bay.to_str().unwrap()),
        "the message names the bay: {err}"
    );
    let v = json(&ff(&fx, &["--json", "switch", "side"]));
    assert_eq!(v["error"]["id"], "branch/checked-out-elsewhere");

    // The bay cannot take main's branch.
    let output = ff_at(&bay, &["switch", "main"]);
    assert!(
        !output.status.success(),
        "the bay took main's branch: {}",
        out(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("'main' is already used by worktree at"),
        "{err}"
    );
    assert!(
        err.contains(fx.path().to_str().unwrap()),
        "the message names the main worktree: {err}"
    );
    let v = json(&ff_at(&bay, &["--json", "switch", "main"]));
    assert_eq!(v["error"]["id"], "branch/checked-out-elsewhere");

    // And neither moved.
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(
        fx.git_in(&bay, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim(),
        "side"
    );
}

/// The migration carries the chain's reflog with the chain: redo's forward
/// scan reads it, so a carry that dropped it would drop `ff redo` too.
#[test]
fn a_pre_worktree_log_is_carried_with_its_reflog() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Several operations, then one step back: the reflog now carries an undo.
    fx.write("b.txt", "b\n");
    ok(&fx, &["commit", "-m", "one"]);
    fx.write("c.txt", "c\n");
    ok(&fx, &["commit", "-m", "two"]);
    ok(&fx, &["undo"]);

    let redo = redo_rows(&fx);
    assert!(
        !redo.is_empty(),
        "ff history offers a redo path before the carry: {redo:?}"
    );
    assert!(
        fx.git(&["reflog", "show", "refs/fufu/wt/main/ops"])
            .contains("fufu: undo to"),
        "the reflog carries the undo"
    );

    let sha = fx
        .git(&["rev-parse", "refs/fufu/wt/main/ops"])
        .trim()
        .to_string();
    let before = fx
        .git(&["reflog", "show", "--format=%H", "refs/fufu/wt/main/ops"])
        .lines()
        .count();

    // Move the chain back to the old name, reflog and all — by file, not
    // `git update-ref`, which would write a fresh one-line reflog and let the
    // carry pass vacuously.
    let git = fx.path().join(".git");
    let ref_src = git.join("refs/fufu/wt/main/ops");
    let log_src = git.join("logs/refs/fufu/wt/main/ops");
    assert!(
        ref_src.is_file(),
        "expected the chain ref to be loose at {ref_src:?}; a packed ref needs a \
         different fixture construction"
    );
    assert!(
        log_src.is_file(),
        "expected the reflog to be loose at {log_src:?}"
    );
    std::fs::create_dir_all(git.join("refs/fufu")).expect("refs/fufu dir");
    std::fs::create_dir_all(git.join("logs/refs/fufu")).expect("logs/refs/fufu dir");
    std::fs::rename(&ref_src, git.join("refs/fufu/ops")).expect("move the ref");
    std::fs::rename(&log_src, git.join("logs/refs/fufu/ops")).expect("move the reflog");

    // Any command migrates on first touch; a read on a settled tree appends
    // nothing on top.
    ok(&fx, &["status"]);

    let gone = fx.try_git(&["rev-parse", "--verify", "refs/fufu/ops"]);
    assert!(!gone.status.success(), "the pre-worktree name must be gone");
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/wt/main/ops"]).trim(),
        sha,
        "the carried chain points where the chain did"
    );
    let after = fx
        .git(&["reflog", "show", "--format=%H", "refs/fufu/wt/main/ops"])
        .lines()
        .count();
    assert!(
        after >= before,
        "the reflog came with the chain: {before} lines before, {after} after"
    );
    assert!(
        fx.git(&["reflog", "show", "refs/fufu/wt/main/ops"])
            .contains("fufu: undo to"),
        "the carried reflog still says where undo went"
    );
    assert_eq!(
        redo_rows(&fx),
        redo,
        "the redo path is the same after the carry — the point of the test"
    );
}

/// The redo path `ff history` offers, as `(distance, id)` — the rows with a
/// negative distance, nothing else.
fn redo_rows(fx: &Fixture) -> Vec<(i64, String)> {
    let body = ok(fx, &["history", "-n", "0", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    value["data"]["steps"]
        .as_array()
        .expect("steps array")
        .iter()
        .filter(|s| s["distance"].as_i64().expect("distance") < 0)
        .map(|s| {
            (
                s["distance"].as_i64().expect("distance"),
                s["id"].as_str().expect("id").to_string(),
            )
        })
        .collect()
}
