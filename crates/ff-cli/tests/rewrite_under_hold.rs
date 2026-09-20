//! A rewrite of a held branch, end to end: what it does to the hold standing
//! there is decided by the hold's kind. A held restack is dropped by a
//! re-aim and named, kept and pointed at the rewritten commit by an absorb,
//! and one `ff undo` takes the rewrite and the hold's change back together;
//! a held lift carries work that is not on the branch yet, so every rewrite
//! of the branch refuses with the standing hold's exits.

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
    fx.set_config("user.name", "Hold Tester");
    fx.set_config("user.email", "hold@test.test");
    fx
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

/// `feature` and `main` edit the same line of the same file from the same
/// base, so a restack of `feature` conflicts and holds; `other` forks from
/// the base with a file of its own, so a re-aim of `feature` onto it is
/// clean. Leaves the fixture standing on `feature`.
fn held_stack(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "other", "main~1"]);
    fx.write("o.txt", "o\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);

    // The conflicting restack holds — the precondition for a standing hold.
    let held = ff(fx, &["restack"]);
    assert_eq!(held.status.code(), Some(3), "{}", out(&held));
}

/// The branch's metadata file, as `ff status` and the hold read it.
fn branch_meta(fx: &Fixture, branch: &str) -> serde_json::Value {
    let path = fx.path().join(".git/fufu/branch").join(branch);
    serde_json::from_str(&std::fs::read_to_string(path).expect("the branch's metadata"))
        .expect("valid json")
}

/// The observed sequence: a re-aim under a held restack drops the hold and
/// says so, status stops showing it, resolve has nothing to resolve, and one
/// undo brings the tip and the hold back together.
#[test]
fn a_reaim_drops_a_held_restack_and_undo_restores_it() {
    let fx = repo();
    held_stack(&fx);
    let f1 = rev(&fx, "feature");

    let reaim = ff(&fx, &["restack", "--onto", "other"]);
    assert_eq!(reaim.status.code(), Some(0), "{}", out(&reaim));
    let text = stdout(&reaim);
    assert!(
        text.contains("dropped the held restack onto main: feature now sits on other"),
        "the drop is named: {text}"
    );
    assert!(text.contains("undo: ff undo"), "{text}");
    assert_ne!(rev(&fx, "feature"), f1, "the re-aim replayed the branch");

    let status = ff(&fx, &["status"]);
    assert!(status.status.success(), "{}", out(&status));
    assert!(
        !stdout(&status).contains("held:"),
        "no hold stands: {}",
        stdout(&status)
    );

    let resolve = ff(&fx, &["--json", "resolve"]);
    assert_eq!(resolve.status.code(), Some(3), "{}", out(&resolve));
    assert_eq!(json(&resolve)["error"]["id"], "held/none");

    let undo = ff(&fx, &["undo"]);
    assert_eq!(undo.status.code(), Some(0), "{}", out(&undo));
    assert_eq!(rev(&fx, "feature"), f1, "undo puts the tip back");
    let status = ff(&fx, &["--json", "status"]);
    assert_eq!(status.status.code(), Some(0), "{}", out(&status));
    assert_eq!(
        json(&status)["data"]["held"]["verb"],
        "restack",
        "and the hold with it"
    );
    assert_eq!(
        branch_meta(&fx, "feature")["held"]["intent"]["onto"],
        "refs/heads/main",
        "aimed where it was"
    );
}

/// An absorb keeps the base, so the held restack stays and status names the
/// commit the absorb rewrote rather than one the branch no longer has.
#[test]
fn an_absorb_under_a_held_restack_keeps_it_on_the_rewritten_commit() {
    let fx = repo();
    held_stack(&fx);
    let f1 = rev(&fx, "feature");

    fx.write("g.txt", "g\n");
    let absorb = ff(&fx, &["absorb", "--into", f1.as_str()]);
    assert_eq!(absorb.status.code(), Some(0), "{}", out(&absorb));
    let new_f1 = rev(&fx, "feature");
    assert_ne!(new_f1, f1);

    let status = ff(&fx, &["status"]);
    assert!(status.status.success(), "{}", out(&status));
    let text = stdout(&status);
    assert!(text.contains("held: ff restack"), "the hold stands: {text}");
    assert!(
        text.contains(&new_f1[..8]),
        "and names the rewritten commit: {text}"
    );
    assert!(
        !text.contains(&f1[..8]),
        "not the one the branch no longer has: {text}"
    );
    assert_eq!(
        branch_meta(&fx, "feature")["held"]["at"]["id"],
        new_f1.as_str()
    );
}

/// A held lift carries the lifted content, which is not on the branch yet:
/// a re-aim under it refuses with the standing hold's exits.
#[test]
fn a_held_lift_refuses_a_reaim() {
    let fx = repo();
    fx.write("f0.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("doc.txt", "v1\n");
    let c1 = fx.commit("c1");
    fx.write("doc.txt", "v2\n");
    fx.commit("c2");
    fx.git(&["switch", "-q", "-c", "other", "main"]);
    fx.write("o.txt", "o\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);
    let tip = rev(&fx, "feature");

    // Lifting doc.txt out of c1 makes c2's edit a modification of nothing.
    let lift = ff(&fx, &["lift", "--from", c1.as_str(), "doc.txt"]);
    assert_eq!(lift.status.code(), Some(3), "{}", out(&lift));

    let reaim = ff(&fx, &["--json", "restack", "--onto", "other"]);
    assert_eq!(reaim.status.code(), Some(3), "{}", out(&reaim));
    let v = json(&reaim);
    assert_eq!(v["error"]["id"], "held/already-held", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("already has a rewrite held"),
        "{v}"
    );
    let exits = v["error"]["exits"].to_string();
    assert!(exits.contains("ff resolve --abandon"), "{exits}");
    assert!(exits.contains("\"ff resolve\""), "{exits}");
    assert_eq!(rev(&fx, "feature"), tip, "the refusal moves no ref");
    assert_eq!(
        branch_meta(&fx, "feature")["held"]["intent"]["verb"],
        "lift",
        "the hold stands"
    );
}
