//! `ff restack`: moving a branch onto a different base, end to end against
//! the real `ff` binary. Covers the replay, the re-aim, the off-branch
//! restack, the JSON envelope, the nothing-happened exit, the refusals, the
//! undo round trip, and the published-remote disclosure.

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

/// The shared stack, leaving the fixture standing on `feature`:
///
/// m1 ─ m2 ─ m3               (main)
///       └─ f1 ─ f2 ─ f3      (feature, with `mid` at f2)
///
/// Distinct files throughout, so the replay is clean.
fn stack(fx: &Fixture) {
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("m.txt", "m\n");
    fx.commit("m");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.write("c.txt", "c\n");
    fx.commit("f3");
    fx.git(&["branch", "mid", &f2]);

    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    fx.commit("m3");

    fx.git(&["switch", "-q", "feature"]);
}

/// Give `feature` and `main` different upstreams, both already holding the
/// branch's tip: a HEAD-derived name is then visibly the wrong one.
fn upstreamed(fx: &Fixture) {
    // gix resolves the tracking ref through the remote's config — no URL and
    // no fetch refspec means no tracking ref at all, so both must exist.
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

    let feature_tip = fx.git(&["rev-parse", "feature"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/feature", &feature_tip]);
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/feature"]);

    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/main", &main_tip]);
    fx.git(&["config", "branch.main.remote", "origin"]);
    fx.git(&["config", "branch.main.merge", "refs/heads/main"]);
}

#[test]
fn restack_replays_and_reports() {
    let fx = repo();
    stack(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("replayed"), "{text}");
    assert!(text.contains("onto main"), "{text}");
    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(feature_before, feature_after, "the tip must move");
}

#[test]
fn restack_json_envelope() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "restack");
    assert_eq!(v["data"]["restack"]["replayed"], 3);
}

#[test]
fn restack_off_branch_leaves_the_worktree() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack", "feature"]);
    assert!(output.status.success(), "{}", out(&output));
    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(feature_before, feature_after, "the named branch must move");
    assert!(
        fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "an off-branch restack must leave the worktree untouched"
    );
}

#[test]
fn restack_onto_records_the_parent() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("o.txt", "o\n");
    fx.commit("other1");
    fx.git(&["switch", "-q", "main"]);

    let output = ff(&fx, &["restack", "feature", "--onto", "other"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("re-aimed"), "{text}");
    assert!(text.contains("onto other"), "{text}");

    // Advance the new base so the next restack has something to replay:
    // standing on it already would read "already on top of".
    fx.git(&["switch", "-q", "other"]);
    fx.write("o2.txt", "o2\n");
    fx.commit("other2");
    fx.git(&["switch", "-q", "main"]);

    let again = ff(&fx, &["restack", "feature"]);
    assert!(again.status.success(), "{}", out(&again));
    let text = stdout(&again);
    assert!(
        text.contains("onto other"),
        "the recorded parent must be the base, not trunk: {text}"
    );
}

#[test]
fn restack_onto_self_is_exit_2() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack", "feature", "--onto", "feature"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "usage/restack-onto-self");
}

#[test]
fn restack_conflict_is_exit_3() {
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

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let err = stderr(&output);
    assert!(
        err.contains("f.txt"),
        "the message must name the path: {err}"
    );

    let v = json(&ff(&fx, &["--json", "restack"]));
    assert_eq!(v["error"]["id"], "held/rewrite-conflict");

    assert_eq!(
        feature_before,
        fx.git(&["rev-parse", "feature"]).trim(),
        "the tip must not move"
    );
}

#[test]
fn restack_missing_branch() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack", "no-such-branch"]);
    assert!(!output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "branch/not-found");
}

#[test]
fn restack_nothing_to_do() {
    let fx = repo();
    stack(&fx);

    let first = ff(&fx, &["restack"]);
    assert!(first.status.success(), "{}", out(&first));

    let second = ff(&fx, &["restack"]);
    assert!(second.status.success(), "{}", out(&second));
    let text = stdout(&second);
    assert!(text.contains("already on top of"), "{text}");
}

#[test]
fn restack_undo_round_trip() {
    let fx = repo();
    stack(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(
        feature_before,
        fx.git(&["rev-parse", "feature"]).trim(),
        "ff undo must put the tip back"
    );
}

#[test]
fn explain_knows_the_new_ids() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let no_base = ff(&fx, &["explain", "restack/no-base"]);
    assert!(no_base.status.success(), "{}", out(&no_base));
    let text = stdout(&no_base);
    assert!(
        text.contains("there is no base to replay this branch onto"),
        "{text}"
    );

    let unrelated = ff(&fx, &["explain", "restack/unrelated"]);
    assert!(unrelated.status.success(), "{}", out(&unrelated));
    let text = stdout(&unrelated);
    assert!(
        text.contains("the branch and its base share no history"),
        "{text}"
    );

    let onto_self = ff(&fx, &["explain", "usage/restack-onto-self"]);
    assert!(onto_self.status.success(), "{}", out(&onto_self));
    let text = stdout(&onto_self);
    assert!(
        text.contains("a branch cannot be restacked onto itself"),
        "{text}"
    );
}

#[test]
fn restack_names_the_branchs_own_remote() {
    let fx = repo();
    stack(&fx);
    upstreamed(&fx);
    fx.git(&["switch", "-q", "main"]);

    let output = ff(&fx, &["restack", "feature"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("origin/feature"),
        "the disclosure must name feature's own remote: {text}"
    );
    assert!(
        !text.contains("origin/main"),
        "a HEAD-derived name would be visibly wrong: {text}"
    );

    // The restack moved feature, so the JSON form gets a fresh fixture.
    let fx = repo();
    stack(&fx);
    upstreamed(&fx);
    fx.git(&["switch", "-q", "main"]);

    let v = json(&ff(&fx, &["--json", "restack", "feature"]));
    assert_eq!(v["data"]["restack"]["published"], 3);
    assert_eq!(v["data"]["restack"]["published_on"], "origin/feature");
}
