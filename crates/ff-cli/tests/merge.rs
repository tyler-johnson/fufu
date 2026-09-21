//! `ff merge <branch>` end to end against the real `ff` binary: a clean
//! merge lands one two-parent commit and exits 0, a conflicting one records
//! the hold and exits 3 for `ff resolve` and `ff done`, the base refuses
//! with `merge/base`, an ancestor with `merge/nothing`, a branch with
//! nothing of its own fast-forwards, and `ff pull` carries the merge.

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
    fx.set_config("user.name", "Merge Tester");
    fx.set_config("user.email", "merge@test.test");
    fx
}

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_string()
}

fn parents(fx: &Fixture, rev: &str) -> Vec<String> {
    fx.git(&["rev-list", "--parents", "-1", rev])
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect()
}

/// Two features linear off main: `feature-b` forked from an older main
/// with `b1`, main moved by `m1`, `feature-a` off that with `a1`. Standing
/// on `feature-a`. Returns (a1, b1).
fn two_features(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    (a1, b1)
}

/// The same shape, with `a1` and `b1` editing the same line of `c.txt`.
fn conflicting_features(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.write("c.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    fx.write("c.txt", "three\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a"]);
    fx.write("a.txt", "a\n");
    fx.write("c.txt", "two\n");
    let a1 = fx.commit("a1");
    (a1, b1)
}

#[test]
fn a_clean_merge_lands_and_undoes() {
    let fx = repo();
    let (a1, b1) = two_features(&fx);

    let output = ff(&fx, &["merge", "feature-b"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    let merge = tip(&fx, "feature-a");
    assert!(
        text.contains(&format!(
            "merged feature-b into feature-a at {}",
            &merge[..8]
        )),
        "got: {text}"
    );
    assert!(text.contains("undo: ff undo"), "got: {text}");
    assert_eq!(parents(&fx, "feature-a"), vec![a1.clone(), b1.clone()]);
    assert_eq!(tip(&fx, "feature-b"), b1);

    let status = stdout(&ff(&fx, &["status"]));
    assert!(!status.contains("held:"), "no hold: {status}");
    let v = json(&ff(&fx, &["--json", "status"]));
    assert_eq!(v["data"]["base"]["name"], "main", "the base is still main");
    assert!(v["data"]["held"].is_null(), "no hold: {v}");

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(tip(&fx, "feature-a"), a1, "undo puts feature-a back");
}

#[test]
fn json_carries_the_report_and_the_undo() {
    let fx = repo();
    two_features(&fx);
    let output = ff(&fx, &["--json", "merge", "feature-b"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "merge");
    assert_eq!(v["data"]["merge"]["branch"], "feature-a");
    assert_eq!(v["data"]["merge"]["target"], "feature-b");
    assert_eq!(v["data"]["merge"]["fast_forward"], false);
    assert_eq!(
        v["data"]["merge"]["parents"].as_array().map(|p| p.len()),
        Some(2)
    );
    assert_eq!(v["data"]["merge"]["commit"], tip(&fx, "feature-a"));
    assert_eq!(v["data"]["undo"], "ff undo");
}

#[test]
fn a_conflict_holds_and_resolve_then_done_lands_it() {
    let fx = repo();
    let (a1, b1) = conflicting_features(&fx);

    let output = ff(&fx, &["merge", "feature-b"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "held: the merge of feature-b conflicts at {}",
            &b1[..8]
        )),
        "got: {text}"
    );
    assert!(text.contains("in 1 file"), "got: {text}");
    assert_eq!(tip(&fx, "feature-a"), a1, "nothing moved");

    let status = stdout(&ff(&fx, &["status"]));
    assert!(status.contains("held:"), "the hold shows: {status}");
    assert!(
        status.contains("feature-b"),
        "the hold names the target: {status}"
    );

    let resolve = ff(&fx, &["resolve"]);
    assert!(resolve.status.success(), "{}", out(&resolve));
    let c = std::fs::read_to_string(fx.path().join("c.txt")).unwrap();
    assert!(
        c.contains("<<<<<<<") && c.contains(">>>>>>>"),
        "markers: {c}"
    );

    fx.write("c.txt", "two and three\n");
    let done = ff(&fx, &["done"]);
    assert!(done.status.success(), "{}", out(&done));
    let text = stdout(&done);
    assert!(
        text.contains("resolved 1 conflict; landed the merge"),
        "got: {text}"
    );
    assert_eq!(parents(&fx, "feature-a"), vec![a1, b1]);
    assert_eq!(fx.git(&["show", "feature-a:c.txt"]), "two and three\n");
}

/// `--resolve`: the hold is recorded and named, the session opens with the
/// markers, the exit is still 3, and `ff done` lands the merge with two
/// parents.
#[test]
fn merge_resolve_opens_the_session_and_done_lands_the_merge() {
    let fx = repo();
    let (a1, b1) = conflicting_features(&fx);

    let output = ff(&fx, &["merge", "feature-b", "--resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "held: the merge of feature-b conflicts at {}",
            &b1[..8]
        )),
        "got: {text}"
    );
    assert!(
        text.contains("resolving 1 conflict in c.txt on ff/"),
        "got: {text}"
    );
    assert!(text.contains("the merge of feature-b"), "got: {text}");
    assert!(
        !text.contains("ff resolve to fix them"),
        "no held block under an open session: {text}"
    );
    let head = fx
        .git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string();
    assert!(head.starts_with("ff/"), "HEAD is on the session: {head}");
    assert_eq!(tip(&fx, "feature-a"), a1, "nothing moved");
    let c = std::fs::read_to_string(fx.path().join("c.txt")).unwrap();
    assert!(
        c.contains("<<<<<<<") && c.contains(">>>>>>>"),
        "markers: {c}"
    );

    fx.write("c.txt", "two and three\n");
    let done = ff(&fx, &["done"]);
    assert!(done.status.success(), "{}", out(&done));
    assert!(
        stdout(&done).contains("resolved 1 conflict; landed the merge"),
        "got: {}",
        stdout(&done)
    );
    assert_eq!(parents(&fx, "feature-a"), vec![a1, b1]);
    assert_eq!(fx.git(&["show", "feature-a:c.txt"]), "two and three\n");
}

#[test]
fn merge_resolve_json_carries_the_hold_and_the_session() {
    let fx = repo();
    conflicting_features(&fx);
    let output = ff(&fx, &["--json", "merge", "feature-b", "--resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert!(v["data"]["merge"].is_null(), "{v}");
    assert_eq!(v["data"]["held"]["verb"], "merge", "{v}");
    assert_eq!(v["data"]["resolve"]["verb"], "merge", "{v}");
    assert_eq!(v["data"]["resolve"]["merging"], "feature-b", "{v}");
    assert_eq!(v["data"]["resolve"]["held"]["verb"], "merge", "{v}");
    assert!(
        v["data"]["resolve"]["session"]
            .as_str()
            .is_some_and(|s| s.starts_with("ff/")),
        "{v}"
    );
    assert_eq!(v["data"]["undo"], "ff undo", "{v}");
}

#[test]
fn json_on_a_conflict_carries_the_hold() {
    let fx = repo();
    conflicting_features(&fx);
    let output = ff(&fx, &["--json", "merge", "feature-b"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert!(v["data"]["merge"].is_null(), "{v}");
    assert_eq!(v["data"]["held"]["verb"], "merge");
    assert_eq!(v["data"]["held"]["branch"], "feature-a");
    assert_eq!(v["data"]["held"]["paths"][0], "c.txt");
}

#[test]
fn the_base_refuses_naming_pull() {
    let fx = repo();
    let (a1, _b1) = two_features(&fx);
    let output = ff(&fx, &["--json", "merge", "main"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "merge/base");
    let exits = v["error"]["exits"].as_array().expect("exits");
    assert!(exits.iter().any(|e| e == "ff pull"), "{exits:?}");
    assert!(exits.iter().any(|e| e == "ff restack"), "{exits:?}");
    assert_eq!(tip(&fx, "feature-a"), a1);

    let prose = ff(&fx, &["merge", "main"]);
    assert_eq!(prose.status.code(), Some(1));
    let text = stderr(&prose);
    assert!(text.contains("main is feature-a's base"), "got: {text}");
    assert!(text.contains("ff pull"), "got: {text}");
}

#[test]
fn a_second_merge_of_the_same_target_is_nothing() {
    let fx = repo();
    two_features(&fx);
    assert!(ff(&fx, &["merge", "feature-b"]).status.success());
    let output = ff(&fx, &["--json", "merge", "feature-b"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "merge/nothing");
}

#[test]
fn a_branch_with_nothing_of_its_own_fast_forwards() {
    let fx = repo();
    fx.write("base.txt", "base\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a", &base]);

    let output = ff(&fx, &["merge", "feature-b"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("fast-forwarded feature-a to feature-b"),
        "got: {text}"
    );
    assert_eq!(tip(&fx, "feature-a"), b1);
    assert_eq!(parents(&fx, "feature-a").len(), 1);

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "{}", out(&undo));
    assert_eq!(tip(&fx, "feature-a"), base);
}

/// A merge `ff merge` wrote is a merge in the branch's range, so under the
/// default pull policy the base step leaves the branch standing; under
/// `replay` it is carried onto the moved base like any commit.
#[test]
fn pull_carries_a_merge_of_another_tree_under_replay() {
    let fx = repo();
    let (_a1, b1) = two_features(&fx);
    assert!(ff(&fx, &["merge", "feature-b"]).status.success());
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature-a"]);
    let before = tip(&fx, "feature-a");

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        stdout(&output).contains("left alone: behind main (auto: its commits hold a merge)"),
        "got: {}",
        stdout(&output)
    );
    assert_eq!(tip(&fx, "feature-a"), before, "standing under auto");

    assert!(ff(&fx, &["config", "pull", "replay"]).status.success());
    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "feature-a"])
            .status
            .success(),
        "feature-a sits on the moved main"
    );
    let merges: Vec<String> = fx
        .git(&["rev-list", "--merges", "feature-a", "^main"])
        .split_whitespace()
        .map(String::from)
        .collect();
    assert_eq!(merges.len(), 1, "one merge on the branch: {merges:?}");
    assert_eq!(parents(&fx, &merges[0])[1], b1, "its second parent is b1");
    assert!(fx.path().join("m2.txt").is_file());
    assert!(fx.path().join("b.txt").is_file());
}
