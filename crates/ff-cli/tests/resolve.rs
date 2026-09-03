//! `ff resolve`: deal with a held rewrite, end to end against the real `ff`
//! binary. Opening a resolution is a success — exit 0 — and names the files
//! and the way out; with nothing held it refuses with `held/none`, which
//! still owes the shell a 3.

use std::path::Path;
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
    fx.set_config("user.name", "Resolve Tester");
    fx.set_config("user.email", "resolve@test.test");
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

    // The conflicting restack holds — the precondition for resolve.
    let held = ff(fx, &["restack"]);
    assert_eq!(held.status.code(), Some(3), "{}", out(&held));
}

#[test]
fn resolve_opens_and_reports() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["resolve"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("f.txt"),
        "the report must name the file: {text}"
    );
    assert!(
        text.contains("ff done"),
        "the report must name the way out: {text}"
    );
}

#[test]
fn resolve_json_envelope() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["--json", "resolve"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "resolve");
    assert_eq!(v["data"]["resolve"]["regions"], 1);
    assert_eq!(v["data"]["resolve"]["files"][0], "f.txt");
}

#[test]
fn resolve_with_nothing_held_is_exit_3() {
    let fx = repo();
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");

    let output = ff(&fx, &["--json", "resolve"]);
    // `held/none` is a refusal, but its id carries the `held/` prefix, so it
    // exits 3 like every other held/ id.
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "held/none");
}

#[test]
fn done_finishes_the_resolution_and_says_where_the_branch_landed() {
    let fx = repo();
    held_stack(&fx);
    assert!(ff(&fx, &["resolve"]).status.success());
    std::fs::write(fx.path().join("f.txt"), "resolved\n").unwrap();

    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("resolved 1 conflict"),
        "the report counts what the reader fixed: {text}"
    );
    assert!(
        text.contains("replayed 1 commit"),
        "and what it replayed: {text}"
    );
    assert!(
        text.contains("feature is now at"),
        "and where the branch ended up: {text}"
    );
    assert!(
        text.contains("undo: ff undo"),
        "one undo takes it all back: {text}"
    );
}

#[test]
fn done_resolution_json_envelope() {
    let fx = repo();
    held_stack(&fx);
    assert!(ff(&fx, &["resolve"]).status.success());
    std::fs::write(fx.path().join("f.txt"), "resolved\n").unwrap();

    let output = ff(&fx, &["--json", "done"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "done");
    assert_eq!(v["data"]["done"]["verb"], "restack");
    assert_eq!(v["data"]["done"]["branch"], "feature");
    assert_eq!(v["data"]["done"]["fixed"], 1);
    assert_eq!(v["data"]["done"]["still_held"], serde_json::Value::Null);
}

#[test]
fn done_with_the_markers_still_standing_is_exit_3() {
    let fx = repo();
    held_stack(&fx);
    assert!(ff(&fx, &["resolve"]).status.success());

    let output = ff(&fx, &["--json", "done"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "held/unresolved");
}

#[test]
fn done_abandon_over_a_resolution_names_the_resolution() {
    let fx = repo();
    held_stack(&fx);
    assert!(ff(&fx, &["resolve"]).status.success());

    let output = ff(&fx, &["done", "--abandon"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("abandoned the resolution on feature"),
        "a resolution is not an editing session, and the report says so: {text}"
    );
}

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_string()
}

/// A hold stops the branches stacked above where they stand; landing it
/// through `ff done` replays them from the landed tip, and says so.
#[test]
fn done_after_a_resolve_resumes_the_cascade_and_the_json_carries_it() {
    let fx = repo();
    held_stack(&fx);
    let started = ff(&fx, &["start", "feature", "-b", "top"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    let back = ff(&fx, &["switch", "feature"]);
    assert!(back.status.success(), "{}", out(&back));

    assert!(ff(&fx, &["resolve"]).status.success());
    std::fs::write(fx.path().join("f.txt"), "resolved\n").unwrap();
    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("feature is now at"), "{text}");
    assert!(
        text.contains("top followed feature: replayed 1 commit(s)"),
        "the subtree the hold stopped resumed: {text}"
    );
    assert_ne!(tip(&fx, "top"), x1.trim(), "top followed");

    // One undo takes the landing and the cascade back together, and puts
    // the session, markers and all, back in the tree.
    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(tip(&fx, "top"), x1.trim());

    std::fs::write(fx.path().join("f.txt"), "resolved\n").unwrap();
    let output = ff(&fx, &["--json", "done"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "done");
    assert_eq!(v["data"]["done"]["verb"], "restack");
    assert_eq!(v["data"]["done"]["still_held"], serde_json::Value::Null);
    let moved = &v["data"]["done"]["cascade"]["moved"];
    assert_eq!(moved[0]["branch"], "top");
    assert_eq!(moved[0]["base"], "feature");
    assert_eq!(moved[0]["replayed"], 1);
    assert_eq!(
        v["data"]["done"]["cascade"]["held"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
