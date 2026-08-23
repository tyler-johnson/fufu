//! `ff watch` — the stream, run through the real `ff` binary.
//!
//! Every test here is bounded by `-n`, which is exactly what that flag is
//! for: a verb whose normal mode is "run until SIGINT" cannot be tested by a
//! suite that has to terminate. `-n 1` needs no child process at all, and
//! the two that do spawn one have a budget that ends the child whichever way
//! the implementation goes — so nothing in this file can hang.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn cmd_at(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
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
        .env_remove("EMAIL");
    cmd
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    cmd_at(&fx.path(), args).output().expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// A repository with an operation log already going, so `ff watch` has a tip
/// to anchor on.
fn started() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.set_config("user.name", "Watch User");
    fx.set_config("user.email", "watch@test");
    // Any verb bootstraps the log; status is the cheapest.
    assert!(ff(&fx, &["status"]).status.success());
    fx
}

fn commit(fx: &Fixture, file: &str, msg: &str) {
    fx.write(file, &format!("{msg}\n"));
    let out = ff(fx, &["commit", "-m", msg]);
    assert!(out.status.success(), "commit failed: {}", stdout(&out));
}

#[test]
fn the_anchor_is_the_first_line_and_n_one_stops_there() {
    let fx = started();

    let out = ff(&fx, &["watch", "-n", "1"]);

    assert!(out.status.success(), "watch exited {:?}", out.status);
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline, got {text:?}"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "watch");
    assert_eq!(v["data"]["motion"], "start");
    assert!(
        v["data"]["tip"].as_str().is_some_and(|id| !id.is_empty()),
        "the anchor must name a tip, got {v}"
    );
}

#[test]
fn an_operation_lands_on_the_stream_as_it_happens() {
    let fx = started();
    let mut child = cmd_at(&fx.path(), &["watch", "-n", "2", "--kind", "op"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ff watch");
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();

    // Reading the anchor line is the synchronization: `start` is emitted only
    // after the watcher has read its tip, so anything that moves the log
    // after this point is genuinely after the anchor. No sleep, and no race.
    let first: serde_json::Value =
        serde_json::from_str(&lines.next().expect("a start line").expect("io")).expect("json");
    assert_eq!(first["data"]["motion"], "start");

    commit(&fx, "b.txt", "second");

    let second: serde_json::Value =
        serde_json::from_str(&lines.next().expect("a landed line").expect("io")).expect("json");
    let status = child.wait().expect("join ff watch");

    assert!(status.success(), "watch exited {status:?}");
    assert_eq!(second["data"]["motion"], "landed");
    // `--kind op` filters the pre-verb capture out, so the first thing to
    // land is the commit itself.
    assert_eq!(second["data"]["op"]["verb"], "commit");
    assert_eq!(second["data"]["op"]["kind"], "op");
}

#[test]
fn a_closed_pipe_is_a_clean_exit_not_a_panic() {
    let fx = started();
    let mut child = cmd_at(&fx.path(), &["watch", "-n", "3", "--kind", "op"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff watch");

    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let first: serde_json::Value =
        serde_json::from_str(&lines.next().expect("a start line").expect("io")).expect("json");
    assert_eq!(first["data"]["motion"], "start");
    // The far end goes away while the writer is still live — `ff watch |
    // head -1`, without a shell pipeline this suite cannot run on Windows.
    drop(lines);

    commit(&fx, "b.txt", "second");
    commit(&fx, "c.txt", "third");

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .expect("io");
    let status = child.wait().expect("join ff watch");

    assert!(
        status.success(),
        "a broken pipe is a clean exit, got {status:?} with stderr {stderr:?}"
    );
    assert!(
        !stderr.contains("panicked"),
        "EPIPE must not panic, got {stderr:?}"
    );
}
