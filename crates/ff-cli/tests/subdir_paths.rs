//! Path positionals read from the current directory, the way git's do.
//!
//! The fixture holds the case that broke: a file in `a/b` whose name also
//! exists at the root, both edited. From `a/b`, `file.ts` is `a/b/file.ts`
//! for every path-taking verb, `..` climbs to the root and no further, and a
//! path that lands on the root means the whole tree.

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
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff")
}

fn out(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn ok_at(dir: &Path, args: &[&str]) -> String {
    let output = ff_at(dir, args);
    assert!(
        output.status.success(),
        "ff {args:?} failed: {}",
        out(&output)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

fn json_at(dir: &Path, args: &[&str]) -> serde_json::Value {
    serde_json::from_str(&ok_at(dir, args)).expect("JSON envelope")
}

const ROOT_EDIT: &str = "root, edited\n";
const SUB_EDIT: &str = "sub, edited\n";

/// Root `file.ts` and `a/b/file.ts`, committed, then a commit touching only
/// the root one, then both edited in the open change.
fn repo() -> (Fixture, PathBuf) {
    let fx = Fixture::new();
    fx.set_config("user.name", "Subdir Tester");
    fx.set_config("user.email", "subdir@test.test");
    fx.write("file.ts", "root\n");
    fx.write("a/b/file.ts", "sub\n");
    fx.commit("init");
    fx.write("file.ts", "root, again\n");
    fx.commit("root only");
    fx.write("file.ts", ROOT_EDIT);
    fx.write("a/b/file.ts", SUB_EDIT);
    let sub = fx.path().join("a/b");
    (fx, sub)
}

fn read(fx: &Fixture, rel: &str) -> String {
    std::fs::read_to_string(fx.path().join(rel)).expect("read file")
}

fn op_count(fx: &Fixture) -> usize {
    json_at(&fx.path(), &["--json", "op", "log", "-n", "0"])["data"]["ops"]
        .as_array()
        .expect("ops")
        .len()
}

fn names(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .collect()
}

#[test]
fn restore_in_a_subdirectory_restores_the_file_there() {
    let (fx, sub) = repo();
    let v = json_at(&sub, &["--json", "restore", "file.ts"]);
    assert_eq!(v["data"]["restored"], serde_json::json!(["a/b/file.ts"]));
    assert_eq!(read(&fx, "a/b/file.ts"), "sub\n");
    assert_eq!(
        read(&fx, "file.ts"),
        ROOT_EDIT,
        "the root namesake keeps its edit"
    );
}

#[test]
fn restore_refuses_a_path_that_matches_nothing_and_writes_nothing() {
    let (fx, sub) = repo();
    let before = op_count(&fx);
    let output = ff_at(&sub, &["--json", "restore", "nope.ts"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(v["error"]["id"], "usage/no-such-path", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("a/b/nope.ts"),
        "the message shows what the path resolved to: {v}"
    );
    assert_eq!(op_count(&fx), before, "a refusal appends no operation");
    assert_eq!(read(&fx, "a/b/file.ts"), SUB_EDIT);
}

#[test]
fn restore_accepts_a_path_only_the_source_knows() {
    let (fx, _) = repo();
    fx.write("gone.txt", "gone\n");
    let had = fx.commit("add gone");
    fx.remove("gone.txt");
    fx.commit("drop gone");
    ok_at(&fx.path(), &["restore", "gone.txt", "--from", &had]);
    assert_eq!(read(&fx, "gone.txt"), "gone\n");
}

#[test]
fn reads_in_a_subdirectory_name_the_file_there() {
    let (_fx, sub) = repo();
    let diff = ok_at(&sub, &["diff", "--name-only", "file.ts"]);
    assert_eq!(names(&diff), ["a/b/file.ts"], "{diff}");
    let show = ok_at(&sub, &["show", "--name-only", "@", "file.ts"]);
    assert!(show.contains("a/b/file.ts"), "{show}");
    assert!(
        !show
            .lines()
            .any(|line| line.trim_end().ends_with(" file.ts")),
        "{show}"
    );
    let log = ok_at(&sub, &["log", "-n", "0", "file.ts"]);
    assert!(log.contains("init"), "{log}");
    assert!(!log.contains("root only"), "{log}");
}

#[test]
fn commit_in_a_subdirectory_closes_the_file_there() {
    let (fx, sub) = repo();
    ok_at(&sub, &["commit", "-m", "x", "file.ts"]);
    let shown = fx.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(shown.trim(), "a/b/file.ts");
    let open = ok_at(&fx.path(), &["diff", "--name-only"]);
    assert_eq!(names(&open), ["file.ts"], "{open}");
}

#[test]
fn absorb_in_a_subdirectory_moves_the_file_there() {
    let (_fx, sub) = repo();
    let v = json_at(&sub, &["--json", "absorb", "file.ts"]);
    assert_eq!(
        v["data"]["move"]["files"],
        serde_json::json!(["a/b/file.ts"])
    );
}

#[test]
fn dot_dot_climbs_to_the_root_and_no_further() {
    let (_fx, sub) = repo();
    let diff = ok_at(&sub, &["diff", "--name-only", "../../file.ts"]);
    assert_eq!(names(&diff), ["file.ts"], "{diff}");

    let output = ff_at(&sub, &["--json", "diff", "../../../x"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(v["error"]["id"], "usage/no-such-path", "{v}");
}

#[test]
fn dot_is_the_current_directory_and_the_root_is_everything() {
    let (fx, sub) = repo();
    let all = ok_at(&fx.path(), &["diff", "--name-only", "."]);
    assert_eq!(names(&all), ["a/b/file.ts", "file.ts"], "{all}");

    ok_at(&sub, &["restore", "."]);
    assert_eq!(read(&fx, "a/b/file.ts"), "sub\n");
    assert_eq!(read(&fx, "file.ts"), ROOT_EDIT);

    ok_at(&fx.path(), &["restore", "."]);
    assert_eq!(read(&fx, "file.ts"), "root, again\n");
}

#[test]
fn dash_c_reads_the_same_as_standing_there() {
    let (fx, _) = repo();
    let v = json_at(&fx.path(), &["-C", "a/b", "--json", "restore", "file.ts"]);
    assert_eq!(v["data"]["restored"], serde_json::json!(["a/b/file.ts"]));
    assert_eq!(read(&fx, "file.ts"), ROOT_EDIT);
}
