//! `ff log` — the path axis: the rows are the commits that touch the named
//! paths, the `@` row is the open change when it touches them too, and a
//! selector that names nothing is refused rather than answered with an
//! empty log.

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
    fx.set_config("user.name", "Log Tester");
    fx.set_config("user.email", "log@test.test");
    fx
}

/// first writes a.txt and b.txt; second modifies a.txt; third modifies b.txt.
fn fixture() -> Fixture {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.commit("second");
    fx.write("b.txt", "b2\n");
    fx.commit("third");
    fx
}

fn rows(out: &Output) -> Vec<String> {
    stdout(out)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// The positional narrows the rows to the commits that touch it, and no
/// `--` separator is needed to mean it.
#[test]
fn a_path_narrows_the_rows_and_needs_no_separator() {
    let fx = fixture();
    let out = ff(&fx, &["log", "--commits", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(rows(&out).len(), 2);
    let out = ff(&fx, &["log", "--commits", "--", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(rows(&out).len(), 2);
}

/// The `@` row appears only when the open change touches the paths.
#[test]
fn the_open_change_row_appears_only_when_the_paths_are_touched() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let a = ff(&fx, &["log", "a.txt"]);
    assert!(a.status.success(), "{}", stderr(&a));
    assert!(stdout(&a).lines().any(|line| line.contains('@')));
    let b = ff(&fx, &["log", "b.txt"]);
    assert!(b.status.success(), "{}", stderr(&b));
    assert!(!stdout(&b).lines().any(|line| line.contains('@')));
}

/// `--commits` drops the `@` row and keeps the path filter.
#[test]
fn commits_drops_the_open_row_and_keeps_the_filter() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["log", "--commits", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stdout(&out).lines().any(|line| line.contains('@')));
    assert_eq!(rows(&out).len(), 2);
}

/// A path that names nothing is refused, and the refusal is not everything.
#[test]
fn a_path_that_names_nothing_is_refused() {
    let fx = fixture();
    let out = ff(&fx, &["log", "bogus.txt"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("bogus.txt"));
    let out = ff(&fx, &["log", "main"]);
    assert!(!out.status.success());
    assert!(ff(&fx, &["log", "a.txt"]).status.success());
}

/// A sentence in the path slot names the two flag-shaped exits, -r and -m.
#[test]
fn a_sentence_in_the_path_slot_names_the_missing_flag() {
    let fx = fixture();
    let out = ff(&fx, &["log", "fix the parser"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("-r"), "{}", err);
    assert!(err.contains("-m"), "{}", err);
}

/// The JSON envelope keeps its keys; the path axis only removes rows.
#[test]
fn json_keeps_its_shape_with_fewer_rows() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let v = json(&ff(&fx, &["log", "--json", "a.txt"]));
    let data = v["data"].as_object().expect("envelope with a data object");
    assert!(data.contains_key("commits"));
    assert!(data.contains_key("open"));
    assert_eq!(data["commits"].as_array().expect("an array").len(), 2);
    assert!(!data["open"].is_null());
    let v = json(&ff(&fx, &["log", "--json", "b.txt"]));
    let data = v["data"].as_object().expect("envelope with a data object");
    assert!(data.contains_key("commits"));
    assert!(data.contains_key("open"));
    assert!(data["commits"].is_array());
    assert!(data["open"].is_null());
}

/// A reader that walks away ends the log, it does not crash it. `ff log
/// --commits` printed through `println!`, which panics when the pipe closes
/// — so `ff log --commits | head` died with "failed printing to stdout"
/// instead of exiting the way git does. Closing the read end before the
/// child writes a byte reproduces it every time; the other log views never
/// had it, because they already write through the pager's writer.
#[test]
fn a_closed_pipe_ends_the_log_rather_than_crashing_it() {
    let fx = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["log", "--commits", "-n", "0"])
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ff");

    // The reader leaves: every write from here on hits a closed pipe.
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait for ff");
    let err = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        !err.contains("panicked"),
        "a closed pipe panicked the log:\n{err}"
    );
    assert!(
        out.status.success(),
        "a closed pipe should exit clean, got {:?}:\n{err}",
        out.status.code()
    );
}
