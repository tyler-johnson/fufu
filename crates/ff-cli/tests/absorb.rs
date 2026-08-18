//! `ff absorb` and `ff lift`: the move-across-commits surface, end to end
//! against the real `ff` binary. Covers the fold and the lift, the restack
//! either forces, the JSON envelope, the nothing-happened exits, the
//! refusals, and the paths filter.

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
    fx.set_config("user.name", "Absorb Tester");
    fx.set_config("user.email", "absorb@test.test");
    fx
}

#[test]
fn absorb_into_head_reports_and_undoes() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");
    let head_before = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    fx.write("f2.txt", "two\n");
    let output = ff(&fx, &["absorb"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("absorbed into"), "{text}");
    assert!(text.contains("undo: ff undo"), "{text}");
    let head_after = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(head_before, head_after, "the absorb must re-point the tip");

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    let head_restored = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(head_before, head_restored, "ff undo must put the tip back");
}

#[test]
fn absorb_into_mid_stack_restacks_and_moves_branches() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");
    fx.git(&["branch", "mid"]);
    fx.write("f3.txt", "three\n");
    fx.commit("c3");

    fx.write("f2.txt", "two, edited\n");
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let output = ff(&fx, &["absorb", "--into", c2.as_str()]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("restacked"), "{text}");
    assert!(text.contains("moved"), "{text}");
    assert!(text.contains("mid"), "{text}");
    let mid_after = fx.git(&["rev-parse", "mid"]).trim().to_string();
    assert_ne!(
        mid_before, mid_after,
        "a branch at the target must come along"
    );
}

#[test]
fn absorb_json_envelope() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    fx.write("f2.txt", "two\n");
    let output = ff(&fx, &["--json", "absorb"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "absorb");
    let absorb = &v["data"]["absorb"];
    let into = absorb["into"].as_str().expect("into is a string");
    let new = absorb["new"].as_str().expect("new is a string");
    assert_ne!(into, new, "the target must re-point");
}

#[test]
fn absorb_nothing_to_absorb_exits_zero() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let output = ff(&fx, &["absorb"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("nothing to absorb"), "{text}");
}

#[test]
fn absorb_conflict_exits_three() {
    let fx = repo();
    fx.write("f.txt", "x\nrest\n");
    fx.commit("base");
    fx.write("f.txt", "A\nrest\n");
    let c1 = fx.commit("c1");
    fx.git(&["branch", "mid"]);
    fx.write("f.txt", "A2\nrest\n");
    fx.commit("c2");

    // All three sides rewrite line 1: c1 did, c2 did it again, and the
    // open change does it a third way, so folding into c1 conflicts.
    fx.write("f.txt", "C\nrest\n");

    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let output = ff(&fx, &["absorb", "--into", c1.as_str()]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let err = stderr(&output);
    assert!(
        err.contains("f.txt"),
        "the message must name the path: {err}"
    );

    let v = json(&ff(&fx, &["--json", "absorb", "--into", c1.as_str()]));
    assert_eq!(v["error"]["id"], "held/rewrite-conflict");

    assert_eq!(
        main_before,
        fx.git(&["rev-parse", "main"]).trim(),
        "the tip must not move"
    );
    assert_eq!(
        mid_before,
        fx.git(&["rev-parse", "mid"]).trim(),
        "the branch must not move"
    );
}

#[test]
fn absorb_into_open_is_usage_error() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let output = ff(&fx, &["--json", "absorb", "--into", "@"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "usage/absorb-into-open");
}

#[test]
fn absorb_paths_limits() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    let output = ff(&fx, &["absorb", "a.txt"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("limited to 1 path(s)"), "{text}");
    assert!(text.contains("still open"), "{text}");
}

#[test]
fn lift_reports_and_grows_the_open_change() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("c1");

    let output = ff(&fx, &["lift", "a.txt"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("lifted out of"), "{text}");

    let status = ff(&fx, &["status"]);
    assert!(status.status.success(), "{}", out(&status));
    let text = out(&status);
    assert!(
        text.contains("a.txt"),
        "the lifted path is open again: {text}"
    );
}

/// Lifting a commit's every change leaves nothing for it to introduce, so
/// the commit is dropped rather than left empty — and the output says so.
#[test]
fn lift_everything_says_the_commit_is_gone() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");

    let output = ff(&fx, &["lift"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("the commit is gone"), "{text}");

    // Not merely announced: the commit really is out of the history.
    let log = fx.git(&["log", "--format=%H", "main"]);
    assert!(
        !log.contains(c1.trim()),
        "c1 must be gone from main, not empty: {log}"
    );
    assert_eq!(
        fx.git(&["rev-list", "--count", "main"]).trim(),
        "1",
        "only the base survives"
    );
}

#[test]
fn explain_knows_the_new_ids() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let conflict = ff(&fx, &["explain", "held/rewrite-conflict"]);
    assert!(conflict.status.success(), "{}", out(&conflict));
    let text = stdout(&conflict);
    assert!(
        text.contains("the rewrite stops at a commit it cannot replay"),
        "{text}"
    );

    let merge = ff(&fx, &["explain", "rewrite/merge-in-range"]);
    assert!(merge.status.success(), "{}", out(&merge));
    let text = stdout(&merge);
    assert!(
        text.contains("a merge commit sits in the range being replayed"),
        "{text}"
    );
}
