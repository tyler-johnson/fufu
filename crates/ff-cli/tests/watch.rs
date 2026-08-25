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

/// A bay off `started()`, through the verb, so its chain floor is laid the
/// way a real `ff worktree add` lays it.
fn bay(fx: &Fixture, name: &str) -> std::path::PathBuf {
    let out = ff(fx, &["worktree", "add", name]);
    assert!(
        out.status.success(),
        "worktree add failed: {}",
        stdout(&out)
    );
    fx.path().join(name)
}

fn json(line: &str) -> serde_json::Value {
    serde_json::from_str(line).expect("valid json")
}

#[test]
fn all_opens_with_one_anchor_per_worktree() {
    let fx = started();
    bay(&fx, "bay-a");

    let out = ff(&fx, &["watch", "--all", "-n", "2"]);

    assert!(out.status.success(), "watch exited {:?}", out.status);
    let text = stdout(&out);
    let lines: Vec<serde_json::Value> = text.lines().map(json).collect();
    assert_eq!(lines.len(), 2, "two worktrees, two anchors: {text:?}");
    let mut trees: Vec<&str> = lines
        .iter()
        .map(|line| {
            assert_eq!(line["data"]["motion"], "start");
            line["data"]["worktree"].as_str().expect("a worktree field")
        })
        .collect();
    trees.sort_unstable();
    assert_eq!(trees, vec!["bay-a", "main"]);
}

#[test]
fn a_single_tree_stream_names_its_worktree_too() {
    let fx = started();

    let out = ff(&fx, &["watch", "-n", "1"]);

    assert!(out.status.success(), "watch exited {:?}", out.status);
    let v = json(stdout(&out).trim_end());
    assert_eq!(v["data"]["motion"], "start");
    // The field rides both modes, so a subscriber parses one shape.
    assert_eq!(v["data"]["worktree"], "main");
}

#[test]
fn work_in_a_bay_reaches_a_stream_started_in_the_main_worktree() {
    let fx = started();
    let bay = bay(&fx, "bay-a");

    // Three lines: two anchors, then the bay's commit. `--kind op` keeps the
    // pre-verb capture off the stream so the third line is the commit.
    let mut child = cmd_at(&fx.path(), &["watch", "--all", "-n", "3", "--kind", "op"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ff watch --all");
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();

    // Both anchors before anything moves: reading them is the
    // synchronization, exactly as the single-tree test uses `start`.
    for _ in 0..2 {
        let anchor = json(&lines.next().expect("an anchor line").expect("io"));
        assert_eq!(anchor["data"]["motion"], "start");
    }

    std::fs::write(bay.join("b.txt"), "the bay's commit\n").expect("write in the bay");
    let out = cmd_at(&bay, &["commit", "-m", "from the bay"])
        .output()
        .expect("spawn ff commit");
    assert!(out.status.success(), "commit failed: {}", stdout(&out));

    let landed = json(&lines.next().expect("a landed line").expect("io"));
    let status = child.wait().expect("join ff watch");

    assert!(status.success(), "watch exited {status:?}");
    assert_eq!(landed["data"]["motion"], "landed");
    assert_eq!(landed["data"]["op"]["verb"], "commit");
    // The whole point: one process, and the line says which tree it came
    // from. Before this, a supervisor ran one `ff watch` per bay.
    assert_eq!(landed["data"]["worktree"], "bay-a");
}

#[test]
fn all_and_since_are_refused_together() {
    let fx = started();

    let out = ff(&fx, &["watch", "--all", "--since", "@"]);

    assert_eq!(out.status.code(), Some(2), "stdout: {}", stdout(&out));
    let stderr = String::from_utf8(out.stderr.clone()).expect("utf-8 stderr");
    assert!(
        stderr.contains("--since") && stderr.contains("--all"),
        "the message must name the combination, got {stderr:?}"
    );
    // The registered id, reached the way any refusal is reached.
    let explained = ff(&fx, &["explain", "usage/bad-flags"]);
    assert!(
        explained.status.success(),
        "explain exited {:?}",
        explained.status
    );
}

#[test]
fn a_closed_pipe_under_all_is_a_clean_exit_not_a_panic() {
    let fx = started();
    let bay = bay(&fx, "bay-a");

    let mut child = cmd_at(&fx.path(), &["watch", "--all", "-n", "9", "--kind", "op"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff watch --all");

    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let anchor = json(&lines.next().expect("an anchor line").expect("io"));
    assert_eq!(anchor["data"]["motion"], "start");
    drop(lines);

    // Enough motion, in both trees, that the writer is certain to try again.
    commit(&fx, "b.txt", "second");
    std::fs::write(bay.join("c.txt"), "third\n").expect("write in the bay");
    let out = cmd_at(&bay, &["commit", "-m", "third"])
        .output()
        .expect("spawn ff commit");
    assert!(out.status.success(), "commit failed: {}", stdout(&out));

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
