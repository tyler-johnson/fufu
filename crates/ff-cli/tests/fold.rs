//! `ff fold`: landing the branch you stand on into another one, end to end
//! against the real `ff` binary. Covers the replay onto a moved target, the
//! fast-forward, the deletion and its trash pointer, the open change riding
//! along, the cascade and its re-aims, the refusals, the undo round trip,
//! the JSON envelope, and `--stay` across two worktrees.

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

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Fold Tester");
    fx.set_config("user.email", "fold@test.test");
    fx
}

fn tip(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-parse", rev]).trim().to_string()
}

fn tip_in(fx: &Fixture, dir: &Path, rev: &str) -> String {
    fx.git_in(dir, &["rev-parse", rev]).trim().to_string()
}

fn branch_exists(fx: &Fixture, name: &str) -> bool {
    !fx.git(&["branch", "--list", name]).trim().is_empty()
}

fn head_branch(fx: &Fixture) -> String {
    fx.git(&["branch", "--show-current"]).trim().to_string()
}

/// A branch's recorded metadata, as fufu keeps it.
fn meta(fx: &Fixture, branch: &str) -> serde_json::Value {
    let path = fx.path().join(".git/fufu/branch").join(branch);
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).expect("branch metadata is json"),
        Err(_) => serde_json::Value::Null,
    }
}

/// The subjects of `rev`'s first-parent history, newest first.
fn subjects(fx: &Fixture, rev: &str) -> Vec<String> {
    fx.git(&["log", "--format=%s", "--first-parent", rev])
        .lines()
        .map(str::to_string)
        .collect()
}

/// The verbs on this worktree's operation log, newest first.
fn op_verbs(dir: &Path) -> Vec<String> {
    let output = ff_at(dir, &["op", "log", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    v["data"]["ops"]
        .as_array()
        .map(|ops| {
            ops.iter()
                .map(|op| op["verb"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The shared stack, leaving the fixture standing on `feature` in the main
/// worktree, with nothing holding `main`:
///
/// m1 ─ m2                    (main)
///       └─ f1 ─ f2 ─ f3      (feature)
///
/// Distinct files throughout, so the replay is clean.
fn stack(fx: &Fixture) {
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("m.txt", "m\n");
    fx.commit("m2");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.write("b.txt", "b\n");
    fx.commit("f2");
    fx.write("c.txt", "c\n");
    fx.commit("f3");
}

/// Trunk moves on: m3 on `main`, HEAD back on `feature`.
fn trunk_moves(fx: &Fixture) {
    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    fx.commit("m3");
    fx.git(&["switch", "-q", "feature"]);
}

/// Give `main` a tracking ref under `origin`, so a remote target exists.
fn upstreamed(fx: &Fixture) {
    fx.git(&[
        "config",
        "remote.origin.url",
        "https://example.test/origin.git",
    ]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    let main_tip = tip(fx, "main");
    fx.git(&["update-ref", "refs/remotes/origin/main", &main_tip]);
}

/// A linked worktree standing on a fresh `bay` cut from `main`, with one
/// commit of its own, and the main worktree left on `main`:
///
/// root ─ m2              (main, held by the main worktree)
///          └─ b1         (bay, held by the linked worktree)
fn bay(fx: &Fixture) -> PathBuf {
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("m.txt", "m\n");
    fx.commit("m2");
    let dir = fx.root().join("bay");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "bay",
        dir.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    fx.git_in(&dir, &["add", "-A"]);
    fx.git_in(&dir, &["commit", "-qm", "b1"]);
    dir
}

#[test]
fn fold_replays_onto_a_moved_trunk_and_deletes_the_branch() {
    let fx = repo();
    stack(&fx);
    trunk_moves(&fx);

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("replayed 3 commit(s) onto main"), "{text}");
    assert!(text.contains("main moved ahead by 3 commit(s)"), "{text}");
    assert!(text.contains("deleted feature"), "{text}");
    assert!(
        text.contains("its timeline moved to refs/fufu/trash/feature"),
        "{text}"
    );
    assert!(text.contains("now on main"), "{text}");
    assert!(text.contains("undo: ff undo"), "{text}");

    assert!(!branch_exists(&fx, "feature"), "the branch must be gone");
    assert_eq!(head_branch(&fx), "main");
    assert_eq!(
        subjects(&fx, "main"),
        ["f3", "f2", "f1", "m3", "m2", "root"],
        "main's history is linear, the branch's commits above trunk's"
    );
    assert!(
        fx.path().join("d.txt").is_file(),
        "the worktree is at the new tip"
    );
    assert!(
        fx.path().join("c.txt").is_file(),
        "the worktree is at the new tip"
    );
    assert!(
        fx.try_git(&["rev-parse", "--verify", "-q", "refs/fufu/trash/feature"])
            .status
            .success(),
        "the branch's pointer into the log is parked under trash"
    );
}

#[test]
fn fold_fast_forwards_when_trunk_did_not_move() {
    let fx = repo();
    stack(&fx);
    let feature = tip(&fx, "feature");

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(!text.contains("replayed"), "nothing to replay: {text}");
    assert!(text.contains("main moved ahead by 3 commit(s)"), "{text}");
    assert_eq!(tip(&fx, "main"), feature, "main fast-forwards to the tip");
    assert!(!branch_exists(&fx, "feature"));
    assert_eq!(head_branch(&fx), "main");
}

#[test]
fn fold_into_a_named_target_leaves_trunk_alone() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "release"]);
    fx.write("r.txt", "r\n");
    fx.commit("r1");
    fx.git(&["switch", "-q", "feature"]);
    let main = tip(&fx, "main");

    let output = ff(&fx, &["fold", "release"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("onto release"), "{text}");
    assert!(text.contains("now on release"), "{text}");
    assert_eq!(tip(&fx, "main"), main, "trunk did not move");
    assert_eq!(
        subjects(&fx, "release"),
        ["f3", "f2", "f1", "r1", "m2", "root"]
    );
    assert_eq!(head_branch(&fx), "release");
    assert!(!branch_exists(&fx, "feature"));
}

#[test]
fn fold_carries_the_open_change_and_leaves_it_open() {
    let fx = repo();
    stack(&fx);
    trunk_moves(&fx);
    fx.write("a.txt", "a edited\n");
    fx.write("new.txt", "new\n");

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("your change is still open"), "{text}");
    assert_eq!(head_branch(&fx), "main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a edited\n"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
        "new\n"
    );
    let status = fx.git(&["status", "--porcelain"]);
    assert!(status.contains("a.txt"), "the edit is still open: {status}");
    assert!(
        status.contains("new.txt"),
        "the new file is still open: {status}"
    );
}

#[test]
fn fold_of_an_anonymous_bay_says_nothing_was_lost() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    let start = ff(&fx, &["start"]);
    assert!(start.status.success(), "{}", out(&start));
    let anon = head_branch(&fx);
    assert!(anon.starts_with("ff/"), "an anonymous branch: {anon}");
    fx.write("a.txt", "a\n");
    let commit = ff(&fx, &["commit", "-m", "a"]);
    assert!(commit.status.success(), "{}", out(&commit));

    let output = ff(&fx, &["fold", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["data"]["fold"]["anonymous"], true);
    assert_eq!(v["data"]["fold"]["source"], anon);
    assert_eq!(v["data"]["fold"]["target"], "main");
    assert!(!branch_exists(&fx, &anon));
    assert_eq!(head_branch(&fx), "main");

    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    assert!(ff(&fx, &["start"]).status.success());
    fx.write("a.txt", "a\n");
    assert!(ff(&fx, &["commit", "-m", "a"]).status.success());
    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("nothing to lose"), "{text}");
    assert!(!text.contains("deleted"), "{text}");
}

#[test]
fn fold_of_a_bay_with_only_an_open_change_lands() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    assert!(ff(&fx, &["start"]).status.success());
    let anon = head_branch(&fx);
    fx.write("wip.txt", "wip\n");
    let main = tip(&fx, "main");

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!("main did not move: {anon} had nothing of its own")),
        "{text}"
    );
    assert!(text.contains("your change is still open"), "{text}");
    assert_eq!(tip(&fx, "main"), main);
    assert_eq!(head_branch(&fx), "main");
    assert!(!branch_exists(&fx, &anon));
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "wip\n"
    );
}

#[test]
fn fold_carries_the_branches_above_and_reaims_them() {
    let fx = repo();
    stack(&fx);
    // A child stacked on feature, recorded as such.
    let start = ff(&fx, &["start", "feature", "-b", "child"]);
    assert!(start.status.success(), "{}", out(&start));
    fx.write("e.txt", "e\n");
    fx.commit("c1");
    fx.git(&["switch", "-q", "feature"]);
    trunk_moves(&fx);
    assert_eq!(meta(&fx, "child")["parent"], "feature");
    let child_before = tip(&fx, "child");

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("child followed main: replayed 1 commit(s)"),
        "the cascade names the target as the base: {text}"
    );
    assert!(text.contains("re-aimed child at main"), "{text}");
    assert_eq!(meta(&fx, "child")["parent"], "main");
    assert_ne!(tip(&fx, "child"), child_before, "the child moved");
    assert_eq!(
        subjects(&fx, "child"),
        ["c1", "f3", "f2", "f1", "m3", "m2", "root"],
        "the child sits on main's new tip"
    );
    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(meta(&fx, "child")["parent"], "feature");
    assert_eq!(tip(&fx, "child"), child_before);
    assert!(branch_exists(&fx, "feature"));
    assert_eq!(head_branch(&fx), "feature");
}

#[test]
fn fold_holds_a_child_that_conflicts_and_exits_3() {
    let fx = repo();
    stack(&fx);
    let start = ff(&fx, &["start", "feature", "-b", "child"]);
    assert!(start.status.success(), "{}", out(&start));
    // The child and m3 both create d.txt, differently.
    fx.write("d.txt", "child's d\n");
    fx.commit("c1");
    fx.git(&["switch", "-q", "feature"]);
    trunk_moves(&fx);
    let child_before = tip(&fx, "child");

    let output = ff(&fx, &["fold"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("now on main"),
        "the fold itself landed: {text}"
    );
    assert!(text.contains("child"), "{text}");
    assert!(!branch_exists(&fx, "feature"));
    assert_eq!(
        tip(&fx, "child"),
        child_before,
        "a held child does not move"
    );
    assert_eq!(
        meta(&fx, "child")["held"]["intent"]["onto"],
        "refs/heads/main",
        "the hold aims at the target, since the source is gone"
    );
    assert_eq!(meta(&fx, "child")["parent"], "main");

    // The hold is resolvable: the replan aims at a branch that exists.
    let switch = ff(&fx, &["switch", "child"]);
    assert!(switch.status.success(), "{}", out(&switch));
    let resolve = ff(&fx, &["resolve"]);
    assert!(resolve.status.success(), "{}", out(&resolve));
}

#[test]
fn fold_refuses_a_conflicting_replay_with_nothing_changed() {
    let fx = repo();
    fx.write("f.txt", "x\nrest\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "A\nrest\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "B\nrest\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);
    let feature = tip(&fx, "feature");
    let main = tip(&fx, "main");

    let output = ff(&fx, &["fold", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "fold/conflict");
    let message = v["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("f.txt"),
        "the refusal names the path: {message}"
    );
    let exits = v["error"]["exits"].to_string();
    assert!(
        exits.contains("ff restack --onto main"),
        "the way out is the held restack: {exits}"
    );

    assert_eq!(tip(&fx, "feature"), feature);
    assert_eq!(tip(&fx, "main"), main);
    assert_eq!(head_branch(&fx), "feature");
    assert!(
        !op_verbs(&fx.path()).iter().any(|v| v == "fold"),
        "a refusal records no fold"
    );
    assert_eq!(meta(&fx, "feature")["held"], serde_json::Value::Null);
}

#[test]
fn fold_into_self_is_exit_2() {
    let fx = repo();
    stack(&fx);
    let output = ff(&fx, &["fold", "feature", "--json"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "usage/fold-into-self");
    assert!(branch_exists(&fx, "feature"));
}

#[test]
fn fold_of_trunk_is_refused() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);
    let output = ff(&fx, &["fold", "feature", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "fold/trunk-source");
    assert_eq!(head_branch(&fx), "main");
}

#[test]
fn fold_into_a_missing_target_mints_nothing() {
    let fx = repo();
    stack(&fx);
    let output = ff(&fx, &["fold", "nope", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "branch/not-found");
    assert!(!branch_exists(&fx, "nope"));
    assert!(branch_exists(&fx, "feature"));
}

#[test]
fn fold_into_a_remote_target_is_refused() {
    let fx = repo();
    stack(&fx);
    upstreamed(&fx);
    let output = ff(&fx, &["fold", "origin/main", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "fold/remote-target");
    assert!(branch_exists(&fx, "feature"));
}

#[test]
fn fold_defaults_to_the_local_trunk_in_a_clone() {
    let fx = Fixture::new_cloned();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["push", "-q", "origin", "main"]);
    // Trunk resolves through origin/HEAD, as a clone's does.
    fx.git(&["remote", "set-head", "origin", "main"]);
    assert!(ff(&fx, &["start"]).status.success());
    let anon = head_branch(&fx);
    fx.write("a.txt", "a\n");
    assert!(ff(&fx, &["commit", "-m", "a"]).status.success());

    let output = ff(&fx, &["fold"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(head_branch(&fx), "main");
    assert_eq!(subjects(&fx, "main"), ["a", "root"]);
    assert!(!branch_exists(&fx, &anon));
}

#[test]
fn fold_of_an_editing_session_is_refused() {
    let fx = repo();
    stack(&fx);
    let f2 = tip(&fx, "feature~1");
    let edit = ff(&fx, &["edit", &f2]);
    assert!(edit.status.success(), "{}", out(&edit));
    let output = ff(&fx, &["fold", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "session/open");
}

#[test]
fn fold_into_a_target_held_elsewhere_points_at_stay() {
    let fx = repo();
    stack(&fx);
    let other = fx.root().join("other");
    fx.git(&["worktree", "add", "-q", other.to_str().unwrap(), "main"]);
    let main = tip(&fx, "main");

    let output = ff(&fx, &["fold"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = stderr(&output);
    assert!(text.contains("ff fold main --stay"), "{text}");
    let v = json(&ff(&fx, &["fold", "--json"]));
    assert_eq!(v["error"]["id"], "branch/checked-out-elsewhere");
    assert_eq!(tip(&fx, "main"), main);
    assert!(branch_exists(&fx, "feature"));
}

#[test]
fn undo_restores_source_target_head_and_the_open_file_then_redo_lands_again() {
    let fx = repo();
    stack(&fx);
    trunk_moves(&fx);
    fx.write("a.txt", "a edited\n");
    let feature = tip(&fx, "feature");
    let main = tip(&fx, "main");

    let fold = ff(&fx, &["fold"]);
    assert!(fold.status.success(), "{}", out(&fold));
    let new_main = tip(&fx, "main");

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(tip(&fx, "feature"), feature, "the source is back");
    assert_eq!(tip(&fx, "main"), main, "the target retreats");
    assert_eq!(head_branch(&fx), "feature");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a edited\n",
        "the open change is back where it was"
    );
    assert!(
        !fx.path().join("d.txt").exists(),
        "the worktree is back below m3"
    );

    let redo = ff(&fx, &["redo"]);
    assert!(redo.status.success(), "{}", out(&redo));
    assert_eq!(tip(&fx, "main"), new_main);
    assert!(!branch_exists(&fx, "feature"));
    assert_eq!(head_branch(&fx), "main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a edited\n"
    );
}

#[test]
fn fold_json_envelope() {
    let fx = repo();
    stack(&fx);
    trunk_moves(&fx);
    let output = ff(&fx, &["fold", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "fold");
    let fold = &v["data"]["fold"];
    assert_eq!(fold["source"], "feature");
    assert_eq!(fold["target"], "main");
    assert_eq!(fold["anonymous"], false);
    assert_eq!(fold["stay"], false);
    assert_eq!(fold["replayed"], 3);
    assert_eq!(fold["advanced"], 3);
    assert_eq!(fold["new_tip"], tip(&fx, "main"));
    assert_eq!(fold["trash_ref"], "refs/fufu/trash/feature");
    assert_eq!(fold["moved_tree"], serde_json::Value::Null);
    assert!(fold["cascade"].is_object());
    assert_eq!(v["data"]["undo"], "ff undo");
}

#[test]
fn explain_knows_the_fold_ids() {
    let fx = repo();
    for id in [
        "fold/trunk-source",
        "fold/remote-target",
        "fold/conflict",
        "fold/other-tree-conflict",
        "usage/fold-into-self",
    ] {
        let output = ff(&fx, &["explain", id]);
        assert!(output.status.success(), "{id}: {}", out(&output));
        assert!(stdout(&output).contains(id), "{id}");
    }
}

// --- --stay ---------------------------------------------------------------

#[test]
fn stay_advances_the_target_in_the_tree_that_holds_it() {
    let fx = repo();
    let bay = bay(&fx);
    let b1 = tip_in(&fx, &bay, "bay");

    let output = ff_at(&bay, &["fold", "--stay"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("main moved ahead by 1 commit(s)"), "{text}");
    assert!(text.contains("advanced main in"), "{text}");
    assert!(
        text.contains("bay now sits on main with nothing of its own"),
        "{text}"
    );
    assert!(text.contains("the other half: ff undo in"), "{text}");

    assert_eq!(tip(&fx, "main"), b1, "main fast-forwarded to the bay's tip");
    assert_eq!(tip_in(&fx, &bay, "bay"), b1);
    assert_eq!(
        fx.git_in(&bay, &["branch", "--show-current"]).trim(),
        "bay",
        "the bay stays on its branch"
    );
    assert_eq!(head_branch(&fx), "main");
    assert!(
        fx.path().join("b.txt").is_file(),
        "main's worktree holds the landed file"
    );
    assert!(
        fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "main's index follows its tip"
    );
    assert_eq!(meta(&fx, "bay")["parent"], "main");
    assert!(op_verbs(&fx.path()).iter().any(|v| v == "fold"));
    assert!(op_verbs(&bay).iter().any(|v| v == "fold"));
}

#[test]
fn stay_replays_onto_a_moved_target_and_keeps_the_open_change() {
    let fx = repo();
    let bay = bay(&fx);
    // Main moves on, and the bay has an open change.
    fx.write("m2.txt", "m2\n");
    fx.commit("m3");
    std::fs::write(bay.join("wip.txt"), "wip\n").unwrap();

    let output = ff_at(&bay, &["fold", "--stay", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let fold = &v["data"]["fold"];
    assert_eq!(fold["stay"], true);
    assert_eq!(fold["replayed"], 1);
    assert_eq!(fold["advanced"], 1);
    assert_eq!(fold["still_open"], true);
    assert_eq!(fold["moved_tree"]["id"], "main");
    assert_eq!(fold["moved_tree"]["still_open"], false);
    assert_eq!(subjects(&fx, "main"), ["b1", "m3", "m2", "root"]);
    assert_eq!(tip_in(&fx, &bay, "bay"), tip(&fx, "main"));
    assert_eq!(
        std::fs::read_to_string(bay.join("wip.txt")).unwrap(),
        "wip\n",
        "the bay's open change rode the replay"
    );
    assert!(
        bay.join("m2.txt").is_file(),
        "the bay's worktree is at the new tip"
    );
}

#[test]
fn stay_carries_the_other_trees_uncommitted_change() {
    let fx = repo();
    let bay = bay(&fx);
    fx.write("root.txt", "root edited in main\n");

    let output = ff_at(&bay, &["fold", "--stay", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["data"]["fold"]["moved_tree"]["still_open"], true);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("root.txt")).unwrap(),
        "root edited in main\n"
    );
    assert!(fx.path().join("b.txt").is_file());
    let status = fx.git(&["status", "--porcelain"]);
    assert!(status.contains("root.txt"), "still open in main: {status}");
    assert!(
        !status.contains("b.txt"),
        "the landed file is clean: {status}"
    );
}

#[test]
fn stay_refuses_when_the_other_trees_change_conflicts() {
    let fx = repo();
    let bay = bay(&fx);
    // The bay rewrites root.txt in a commit; main edits it, uncommitted.
    std::fs::write(bay.join("root.txt"), "root from the bay\n").unwrap();
    fx.git_in(&bay, &["commit", "-qam", "b2"]);
    fx.write("root.txt", "root edited in main\n");
    let main = tip(&fx, "main");
    let bay_tip = tip_in(&fx, &bay, "bay");
    let index = fx.index_bytes();

    let output = ff_at(&bay, &["fold", "--stay", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "fold/other-tree-conflict");
    let message = v["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("root.txt"), "{message}");

    assert_eq!(tip(&fx, "main"), main);
    assert_eq!(tip_in(&fx, &bay, "bay"), bay_tip);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("root.txt")).unwrap(),
        "root edited in main\n"
    );
    assert_eq!(fx.index_bytes(), index, "main's index is untouched");
    assert!(!op_verbs(&fx.path()).iter().any(|v| v == "fold"));
    assert!(!op_verbs(&bay).iter().any(|v| v == "fold"));
    assert_eq!(meta(&fx, "bay")["parent"], serde_json::Value::Null);
}

#[test]
fn stay_undo_in_the_bay_restores_the_source_and_names_the_other_tree() {
    let fx = repo();
    let bay = bay(&fx);
    let b1 = tip_in(&fx, &bay, "bay");
    fx.write("m2.txt", "m2\n");
    fx.commit("m3");
    let main_moved = tip(&fx, "main");
    assert!(ff_at(&bay, &["fold", "--stay"]).status.success());
    let landed = tip(&fx, "main");
    assert_ne!(landed, main_moved);

    let undo = ff_at(&bay, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    let text = stdout(&undo);
    assert!(
        text.contains("main keeps the commits"),
        "the undo names the other half: {text}"
    );
    assert!(
        ff_testsupport::paths::names(&text, &fx.path()),
        "the undo names the other tree's path: {text}"
    );
    assert_eq!(tip_in(&fx, &bay, "bay"), b1, "the source is back");
    assert_eq!(tip(&fx, "main"), landed, "the target keeps the commits");
    assert_eq!(meta(&fx, "bay")["parent"], serde_json::Value::Null);
    assert!(
        fx.path().join("b.txt").is_file(),
        "main's worktree is untouched"
    );

    // The other half: undo in main retreats the target and its files.
    let undo_main = ff(&fx, &["undo"]);
    assert!(undo_main.status.success(), "{}", out(&undo_main));
    let text = stdout(&undo_main);
    assert!(
        text.contains("bay keeps sitting on the new tip"),
        "the undo names the other half: {text}"
    );
    assert_eq!(tip(&fx, "main"), main_moved);
    assert!(!fx.path().join("b.txt").exists(), "main's files retreated");
    assert_eq!(tip_in(&fx, &bay, "bay"), b1, "the bay is untouched");
}

#[test]
fn stay_undo_in_the_other_tree_first_then_the_bay_converges() {
    let fx = repo();
    let bay = bay(&fx);
    let b1 = tip_in(&fx, &bay, "bay");
    fx.write("m2.txt", "m2\n");
    fx.commit("m3");
    let main_moved = tip(&fx, "main");
    std::fs::write(bay.join("wip.txt"), "wip\n").unwrap();
    assert!(ff_at(&bay, &["fold", "--stay"]).status.success());
    let landed = tip(&fx, "main");

    let undo_main = ff(&fx, &["undo"]);
    assert!(undo_main.status.success(), "{}", out(&undo_main));
    assert_eq!(tip(&fx, "main"), main_moved, "the target retreats");
    assert!(!fx.path().join("b.txt").exists());
    assert_eq!(tip_in(&fx, &bay, "bay"), landed, "the bay is untouched");
    assert_eq!(meta(&fx, "bay")["parent"], "main");

    let undo_bay = ff_at(&bay, &["undo"]);
    assert!(undo_bay.status.success(), "{}", out(&undo_bay));
    assert_eq!(tip_in(&fx, &bay, "bay"), b1);
    assert_eq!(tip(&fx, "main"), main_moved);
    assert_eq!(meta(&fx, "bay")["parent"], serde_json::Value::Null);
    assert_eq!(
        std::fs::read_to_string(bay.join("wip.txt")).unwrap(),
        "wip\n",
        "the bay's open change is back where it was"
    );
    assert!(
        !bay.join("m2.txt").exists(),
        "the bay's worktree is back below m3"
    );
}

#[test]
fn stay_with_the_other_tree_mid_command_exits_4() {
    let fx = repo();
    let bay = bay(&fx);
    let main = tip(&fx, "main");

    // Hold main's chain lock the way a running fufu writer would: the marker
    // file `ops::lock` acquires, already present.
    let fufu = fx.path().join(".git/fufu");
    std::fs::create_dir_all(&fufu).unwrap();
    let lock = fufu.join("oplog-main.lock");
    std::fs::write(&lock, "held by the test").unwrap();

    let output = ff_at(&bay, &["fold", "--stay", "--json"]);
    assert_eq!(output.status.code(), Some(4), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "ref/contended");
    assert_eq!(tip(&fx, "main"), main, "nothing was written");
    assert!(!op_verbs(&bay).iter().any(|v| v == "fold"));

    std::fs::remove_file(&lock).unwrap();
    let output = ff_at(&bay, &["fold", "--stay"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(tip(&fx, "main"), tip_in(&fx, &bay, "bay"));
}

#[test]
fn stay_with_no_holder_keeps_the_branch_here() {
    let fx = repo();
    stack(&fx);
    trunk_moves(&fx);
    fx.write("wip.txt", "wip\n");

    let output = ff(&fx, &["fold", "--stay", "--json"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let fold = &v["data"]["fold"];
    assert_eq!(fold["stay"], true);
    assert_eq!(fold["moved_tree"], serde_json::Value::Null);
    assert_eq!(fold["trash_ref"], serde_json::Value::Null);
    assert!(branch_exists(&fx, "feature"), "the branch is kept");
    assert_eq!(head_branch(&fx), "feature", "this worktree stays put");
    assert_eq!(tip(&fx, "feature"), tip(&fx, "main"));
    assert_eq!(
        subjects(&fx, "main"),
        ["f3", "f2", "f1", "m3", "m2", "root"]
    );
    assert_eq!(meta(&fx, "feature")["parent"], "main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "wip\n"
    );
    assert!(
        fx.path().join("d.txt").is_file(),
        "the worktree is at the new tip"
    );

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(subjects(&fx, "main"), ["m3", "m2", "root"]);
    assert_eq!(subjects(&fx, "feature"), ["f3", "f2", "f1", "m2", "root"]);
    assert_eq!(meta(&fx, "feature")["parent"], serde_json::Value::Null);
}
