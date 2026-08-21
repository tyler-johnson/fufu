//! `ff commit <paths>` — close a slice, leave the rest open.
//!
//! The property that matters is not that a commit lands: it is that the close
//! is a *slice*. The selected paths land in the commit, the unselected ones
//! stay the open change, and `ff undo` takes the whole thing back. The halves
//! are asserted separately, because that is what catches a tree / index_tree
//! swap a whole-tree test would not see.

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

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Commit Paths Tester");
    fx.set_config("user.email", "commit-paths@test.test");
    fx
}

/// A closed slice carries only the selected file; the other stays open,
/// visible in status and in the diff.
#[test]
fn a_slice_lands_and_the_rest_stays_open() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");

    let out = ff(&fx, &["commit", "a.txt", "-m", "a: second"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // HEAD carries the new a.txt and the old b.txt.
    assert_eq!(fx.git(&["show", "HEAD:a.txt"]), "a2\n");
    assert_eq!(fx.git(&["show", "HEAD:b.txt"]), "b1\n");

    // b.txt is still the open change; a.txt is not.
    let text = stdout(&ff(&fx, &["status"]));
    assert!(text.contains("b.txt"), "b.txt stays open: {text}");
    assert!(!text.contains("a.txt"), "a.txt is closed: {text}");

    // The diff touches b.txt only.
    let body = stdout(&ff(&fx, &["diff"]));
    assert!(body.contains("b/b.txt"), "the diff names b.txt: {body}");
    assert!(
        !body.contains("b/a.txt"),
        "the diff does not name a.txt: {body}"
    );
}

/// `ff undo` after a slice restores both halves — the round trip that proves
/// the tree / index_tree pair was not swapped. The worktree files are
/// compared byte-for-byte; the index is compared by the tree it points at
/// (`git write-tree`), because the raw index bytes carry per-entry stat
/// timestamps that the restore legitimately rewrites and so cannot be held
/// byte-equal.
#[test]
fn undo_restores_both_halves_byte_for_byte() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");

    let a_before = std::fs::read(fx.path().join("a.txt")).unwrap();
    let b_before = std::fs::read(fx.path().join("b.txt")).unwrap();
    let tree_before = fx.git(&["write-tree"]).trim().to_string();

    let out = ff(&fx, &["commit", "a.txt", "-m", "slice a"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let out = ff(&fx, &["undo"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    assert_eq!(std::fs::read(fx.path().join("a.txt")).unwrap(), a_before);
    assert_eq!(std::fs::read(fx.path().join("b.txt")).unwrap(), b_before);
    assert_eq!(fx.git(&["write-tree"]).trim(), tree_before);
}

/// The slice is what HEAD carries and the remainder is the open change — the
/// change_stat half of a partial close, stated as a test.
#[test]
fn the_slice_is_what_head_carries_and_the_remainder_is_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");

    let out = ff(&fx, &["commit", "a.txt", "-m", "slice a"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let v = json(&ff(&fx, &["status", "--json"]));
    let paths: Vec<&str> = v["data"]["changes"]
        .as_array()
        .expect("changes array")
        .iter()
        .map(|f| f["path"].as_str().expect("path"))
        .collect();
    assert_eq!(
        paths,
        vec!["b.txt"],
        "the open change is exactly the remainder"
    );
}

/// A clean slice refuses the way a clean tree does, but the refusal is
/// scoped: a dirty sibling still lands.
#[test]
fn a_clean_slice_refuses_with_commit_empty() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("b.txt", "b2\n");

    let out = ff(&fx, &["commit", "a.txt", "-m", "slice a"]);
    assert!(!out.status.success(), "a clean slice closes nothing");
    let err = stderr(&out);
    assert!(err.contains("a.txt"), "names the clean path: {err}");

    let out = ff(&fx, &["commit", "b.txt", "-m", "slice b"]);
    assert!(
        out.status.success(),
        "the dirty sibling still lands: {}",
        stderr(&out)
    );
}

/// -b is orthogonal to the slice: it decides where the close lands, not what
/// it carries.
#[test]
fn a_slice_composes_with_b() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");

    let out = ff(&fx, &["commit", "a.txt", "-b", "sliced", "-m", "slice a"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "sliced"
    );

    let text = stdout(&ff(&fx, &["status"]));
    assert!(text.contains("b.txt"), "the other file stays open: {text}");
}

/// A sentence in the path slot is a forgotten -m: it is named, pointed at the
/// flag, and nothing is written.
#[test]
fn a_forgotten_m_is_named_rather_than_matched() {
    let fx = repo();
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");

    let head_before = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let out = ff(&fx, &["commit", "fix the parser"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("fix the parser"), "names the token: {err}");
    assert!(err.contains("-m"), "points at the flag: {err}");

    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        head_before,
        "a refusal writes nothing"
    );
}
