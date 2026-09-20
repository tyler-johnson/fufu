//! `ff resolve`'s merge door, end to end against the real `ff` binary: on a
//! branch with no hold whose commits hold a merge of its base, resolve takes
//! the base in by one merge commit and exits 0; a conflicting auto-merge
//! records the hold and opens the session in one step, exit 3; `ff done`
//! lands the merge; a linear branch refuses naming `ff restack`.

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

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_string()
}

fn head_branch(fx: &Fixture) -> String {
    fx.git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string()
}

/// Standing on `side`, whose commits hold a merge of `main`, with `main`
/// moved again since on a file of its own.
fn merge_holding_side(fx: &Fixture) {
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "side"]);
    fx.write("s1.txt", "s1\n");
    fx.commit("s1");
    fx.git(&["merge", "-q", "--no-edit", "main"]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "side"]);
}

/// The same shape, with `side` and `main` editing the same line of `c.txt`
/// since the merge, so the auto-merge conflicts there.
fn conflicting_side(fx: &Fixture) {
    fx.write("base.txt", "base\n");
    fx.write("c.txt", "one\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "side"]);
    fx.write("s1.txt", "s1\n");
    fx.commit("s1");
    fx.git(&["merge", "-q", "--no-edit", "main"]);
    fx.write("c.txt", "two\n");
    fx.commit("s2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("c.txt", "three\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "side"]);
}

#[test]
fn a_clean_merge_lands_and_undoes() {
    let fx = repo();
    merge_holding_side(&fx);
    let side_before = tip(&fx, "side");
    let main_tip = tip(&fx, "main");

    let output = ff(&fx, &["resolve"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    let merge = tip(&fx, "side");
    assert!(
        text.contains(&format!("merged main into side at {}", &merge[..8])),
        "got: {text}"
    );
    assert!(text.contains("undo: ff undo"), "got: {text}");
    let parents: Vec<String> = fx
        .git(&["rev-list", "--parents", "-1", "side"])
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect();
    assert_eq!(parents, vec![side_before.clone(), main_tip]);

    let status = stdout(&ff(&fx, &["status"]));
    assert!(!status.contains("base moved"), "up to date now: {status}");
    assert!(!status.contains("held:"), "no hold: {status}");

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(tip(&fx, "side"), side_before, "undo puts side back");
}

#[test]
fn a_conflicting_merge_holds_and_opens_at_exit_3() {
    let fx = repo();
    conflicting_side(&fx);
    let m2 = tip(&fx, "main");
    let side_before = tip(&fx, "side");

    let output = ff(&fx, &["resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "held: the merge of main conflicts at {} in 1 file",
            &m2[..8]
        )),
        "got: {text}"
    );
    let session = head_branch(&fx);
    assert_ne!(session, "side", "HEAD is on the session");
    assert!(
        text.contains(&format!("resolving 1 conflict in c.txt on {session}")),
        "got: {text}"
    );
    assert!(text.contains("the merge of main"), "got: {text}");
    assert_eq!(tip(&fx, "side"), side_before, "side's tip stands");

    // On the session, status names the resolution by the merge.
    let status = stdout(&ff(&fx, &["status"]));
    assert!(status.contains("from the merge of main"), "got: {status}");

    // Back on `side`, the hold reads as the merge of main.
    assert!(ff(&fx, &["switch", "side"]).status.success());
    let status = stdout(&ff(&fx, &["status"]));
    assert!(
        status.contains("held: the merge of main conflicts"),
        "got: {status}"
    );
}

#[test]
fn the_json_carries_the_hold_inside_resolve() {
    let fx = repo();
    conflicting_side(&fx);

    let output = ff(&fx, &["--json", "resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["data"]["resolve"]["verb"], "merge");
    assert_eq!(v["data"]["resolve"]["held"]["verb"], "merge");
    assert_eq!(v["data"]["resolve"]["held"]["branch"], "side");
    assert_eq!(v["data"]["resolve"]["merging"], "main");
    assert_eq!(v["data"]["undo"], "ff undo");
}

#[test]
fn done_lands_the_merge() {
    let fx = repo();
    conflicting_side(&fx);
    assert_eq!(ff(&fx, &["resolve"]).status.code(), Some(3));
    std::fs::write(fx.path().join("c.txt"), "two and three\n").unwrap();

    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("resolved 1 conflict; landed the merge"),
        "got: {text}"
    );
    assert!(text.contains("side is now at"), "got: {text}");
    assert!(text.contains("back on side"), "got: {text}");
    assert_eq!(head_branch(&fx), "side");
    let parents = fx.git(&["rev-list", "--parents", "-1", "side"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "two parents: {parents}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c.txt")).unwrap(),
        "two and three\n"
    );
}

#[test]
fn done_abandon_closes_the_merge_session_and_keeps_the_hold() {
    let fx = repo();
    conflicting_side(&fx);
    let side_before = tip(&fx, "side");
    assert_eq!(ff(&fx, &["resolve"]).status.code(), Some(3));

    let output = ff(&fx, &["done", "--abandon"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("closed the resolution of side: the merge of main is still held"),
        "got: {text}"
    );
    assert_eq!(tip(&fx, "side"), side_before, "the tip stands");
    assert_eq!(head_branch(&fx), "side");
    let status = stdout(&ff(&fx, &["status"]));
    assert!(status.contains("held"), "the hold stands: {status}");

    let output = ff(&fx, &["resolve", "--abandon"]);
    assert!(output.status.success(), "{}", out(&output));
    let status = stdout(&ff(&fx, &["status"]));
    assert!(!status.contains("held:"), "the hold went with it: {status}");
}

#[test]
fn a_linear_branch_refuses_naming_restack() {
    let fx = repo();
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("g.txt", "g\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    let output = ff(&fx, &["--json", "resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "held/none");
    let message = v["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("ff restack"), "got: {message}");
}
