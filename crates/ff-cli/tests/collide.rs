//! `ff collide` — the sideways axis, run through the real `ff` binary.
//! A collision is a finding, not a failure: every test here expects exit 0.

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

/// Three branches off one base: `feat-a` and `feat-b` both edit the same
/// line of `shared.txt` differently; `fix-c` adds only its own new file.
fn three_branches() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "alpha\nbeta\ngamma\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "feat-a"]);
    fx.write("shared.txt", "alpha\nBETA-A\ngamma\n");
    fx.commit("feat-a edits the middle line");
    fx.git(&["switch", "main"]);
    fx.git(&["switch", "-c", "feat-b"]);
    fx.write("shared.txt", "alpha\nBETA-B\ngamma\n");
    fx.commit("feat-b edits the middle line");
    fx.git(&["switch", "main"]);
    fx.git(&["switch", "-c", "fix-c"]);
    fx.write("c.txt", "c\n");
    fx.commit("fix-c adds its own file");
    fx.git(&["switch", "main"]);
    fx
}

#[test]
fn the_human_line_marks_a_collision() {
    let fx = three_branches();
    let out = ff(&fx, &["collide"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let line = text
        .lines()
        .find(|line| line.contains("feat-a") && line.contains("feat-b"))
        .expect("a line naming both branches");
    assert!(
        line.contains("shared.txt"),
        "the collision names its path: {line:?}"
    );
}

#[test]
fn a_clear_pair_reads_clear() {
    let fx = three_branches();
    let out = ff(&fx, &["collide"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let line = text
        .lines()
        .find(|line| line.contains("feat-a") && line.contains("fix-c"))
        .expect("a line naming the clear pair");
    assert!(line.contains('\u{2713}'), "the clear glyph: {line:?}");
    assert!(
        !line.contains("shared.txt"),
        "a clear pair carries no path list: {line:?}"
    );
}

#[test]
fn the_clear_set_is_printed() {
    let fx = three_branches();
    let out = ff(&fx, &["collide"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let line = text
        .lines()
        .find(|line| line.contains("clear set:"))
        .expect("the clear set line");
    assert!(
        line.contains("fix-c"),
        "the clear set names fix-c: {line:?}"
    );
}

#[test]
fn uncommitted_work_is_marked_and_explained() {
    let fx = three_branches();
    fx.git(&["switch", "feat-b"]);
    fx.write("shared.txt", "alpha\nBETA-B, OPEN\ngamma\n");
    // Every `ff` command captures first, so this is how the open change
    // reaches the operation log — not by hand-building that state.
    let capture = ff(&fx, &["status"]);
    assert!(capture.status.success());
    let out = ff(&fx, &["collide"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.lines().any(|line| line.contains("feat-b*")),
        "the open branch is suffixed with a star: {text:?}"
    );
    assert!(
        text.contains("* has uncommitted work"),
        "the legend explains the star: {text:?}"
    );
}

#[test]
fn json_carries_the_whole_model() {
    let fx = three_branches();
    let out = ff(&fx, &["collide", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert!(v["ff"].is_number(), "ff envelope key present");
    assert_eq!(v["cmd"], "collide");

    let data = &v["data"];
    assert!(data.get("sides").is_some(), "data has sides");
    assert!(data.get("pairs").is_some(), "data has pairs");
    assert!(data.get("clear").is_some(), "data has clear");

    let collide = data["pairs"]
        .as_array()
        .expect("pairs is an array")
        .iter()
        .find(|pair| pair["pairing"]["kind"] == "collide")
        .expect("at least one colliding pair");
    assert_eq!(
        collide["pairing"]["paths"],
        serde_json::json!(["shared.txt"])
    );
}

#[test]
fn one_branch_is_nothing_to_compare() {
    let fx = Fixture::new();
    fx.write("shared.txt", "alpha\nbeta\ngamma\n");
    fx.commit("base");
    let out = ff(&fx, &["collide"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("nothing to compare"));
}
