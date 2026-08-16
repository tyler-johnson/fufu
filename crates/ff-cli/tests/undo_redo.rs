//! `ff undo` and `ff redo` end to end.
//!
//! The script this file was written from, before the implementation existed:
//!
//! ```text
//! ff -m a && ff -m b && ff -m c    # one run, no session
//! ff undo                          # all three go, as one step
//! ff redo                          # all three come back
//! ff --session s -m d && ff -m e   # different sessions: two runs
//! ff undo                          # only e
//! ```
//!
//! A capture is a machine's granularity and a person's undo is not, so the
//! step size is a *run*: the longest stretch of adjacent captures carrying
//! the same session. The session is only the equality test — no session
//! compares equal to no session — which is what keeps sessions tags rather
//! than ranges.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_env(dir: &Path, args: &[&str], session: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
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
        .env_remove("EMAIL");
    match session {
        Some(name) => cmd.env("FF_SESSION", name),
        None => cmd.env_remove("FF_SESSION"),
    };
    cmd.output().expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_env(&fx.path(), args, None)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn ok(fx: &Fixture, args: &[&str]) -> String {
    let out = ff(fx, args);
    assert!(out.status.success(), "ff {args:?} failed: {}", stderr(&out));
    stdout(&out)
}

fn body(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.path().join("a.txt")).expect("read a.txt")
}

fn base() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Undo Tester");
    fx.set_config("user.email", "undo@test.test");
    fx.write("a.txt", "start\n");
    fx.commit("init");
    fx
}

/// Three adjacent untagged captures are one run, and one `ff undo` steps over
/// all three. Redo brings the whole run back.
#[test]
fn a_run_of_captures_is_one_step_in_both_directions() {
    let fx = base();
    for letter in ["a", "b", "c"] {
        fx.write("a.txt", &format!("{letter}\n"));
        ok(&fx, &["-m", letter]);
    }
    assert_eq!(body(&fx), "c\n");

    let text = ok(&fx, &["undo"]);
    assert_eq!(body(&fx), "start\n", "all three went as one step: {text}");
    assert!(text.starts_with("undid"), "{text}");

    let text = ok(&fx, &["redo"]);
    assert_eq!(body(&fx), "c\n", "and all three came back: {text}");
    assert!(text.starts_with("redid"), "{text}");
}

/// A session boundary ends a run. `None` never joins a tag, so the untagged
/// capture is its own step and the tagged one waits behind it.
#[test]
fn a_session_boundary_ends_the_run() {
    let fx = base();
    fx.write("a.txt", "d\n");
    let out = ff_env(&fx.path(), &["-m", "d"], Some("s"));
    assert!(out.status.success(), "{}", stderr(&out));
    fx.write("a.txt", "e\n");
    ok(&fx, &["-m", "e"]);

    ok(&fx, &["undo"]);
    assert_eq!(body(&fx), "d\n", "only e went");
    ok(&fx, &["undo"]);
    assert_eq!(body(&fx), "start\n", "then d");
}

/// A verb's operation is a decision somebody made, so it is always its own
/// step and always ends a run — which is what keeps undo from stepping past a
/// commit by accident.
///
/// The step *between* the two closes is the pre-close capture of the second,
/// and it is a run of its own by the same rule. Stepping over it moves no ref
/// and is not nothing: the working tree goes back to what the first close
/// left, which is exactly the state that capture was taken against.
#[test]
fn a_commit_is_always_its_own_step() {
    let fx = base();
    fx.write("a.txt", "first\n");
    ok(&fx, &["commit", "-m", "one"]);
    fx.write("a.txt", "second\n");
    ok(&fx, &["commit", "-m", "two"]);
    assert_eq!(fx.git(&["log", "--oneline"]).lines().count(), 3);

    let commits = || fx.git(&["log", "--oneline"]).lines().count();

    let text = ok(&fx, &["undo"]);
    assert_eq!(commits(), 2, "one close, never both: {text}");
    assert!(text.contains("two"), "named by what came back: {text}");
    assert_eq!(body(&fx), "second\n", "the change it closed is open again");

    let text = ok(&fx, &["undo"]);
    assert_eq!(
        commits(),
        2,
        "the capture between them moved no ref: {text}"
    );
    assert_eq!(body(&fx), "first\n", "but the tree went back with it");

    let text = ok(&fx, &["undo"]);
    assert_eq!(commits(), 1, "and then the first close: {text}");
    assert!(text.contains("one"), "{text}");
}

/// Undo moves a pointer; it does not write an operation. So the log records
/// work and never navigation, and a round trip leaves it byte-identical.
#[test]
fn a_round_trip_writes_nothing_and_the_reflog_shows_the_moves() {
    let fx = base();
    fx.write("a.txt", "work\n");
    ok(&fx, &["commit", "-m", "landed"]);

    let tip = fx.git(&["rev-parse", "refs/fufu/ops"]);
    let count = ok(&fx, &["op", "log", "--captures", "-n", "0"])
        .lines()
        .count();

    ok(&fx, &["undo"]);
    ok(&fx, &["redo"]);

    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/ops"]),
        tip,
        "back exactly where it started"
    );
    assert_eq!(
        ok(&fx, &["op", "log", "--captures", "-n", "0"])
            .lines()
            .count(),
        count,
        "and nothing was appended saying so"
    );

    // Where the pointer has been is recorded where git already keeps such
    // things.
    let reflog = fx.git(&["reflog", "show", "refs/fufu/ops"]);
    assert!(reflog.contains("fufu: undo to"), "{reflog}");
    assert!(reflog.contains("fufu: redo to"), "{reflog}");
}

/// Repeatable in both directions: each undo goes one run further back, each
/// redo one run further forward, and they meet where they started.
#[test]
fn both_verbs_repeat() {
    let fx = base();
    for letter in ["a", "b", "c"] {
        fx.write("a.txt", &format!("{letter}\n"));
        ok(&fx, &["-m", letter]);
        // A commit between the captures, so each is its own run.
        ok(&fx, &["commit", "-m", &format!("close {letter}")]);
    }
    let commits = fx.git(&["log", "--oneline"]).lines().count();

    ok(&fx, &["undo"]);
    ok(&fx, &["undo"]);
    assert!(fx.git(&["log", "--oneline"]).lines().count() < commits);

    ok(&fx, &["redo"]);
    ok(&fx, &["redo"]);
    assert_eq!(
        fx.git(&["log", "--oneline"]).lines().count(),
        commits,
        "forward all the way back"
    );
    assert_eq!(body(&fx), "c\n");
}

/// New work after an undo forks the log rather than truncating it: nothing is
/// discarded, and redo stops offering a path it can no longer take.
#[test]
fn redo_refuses_once_work_has_landed() {
    let fx = base();
    fx.write("a.txt", "work\n");
    ok(&fx, &["commit", "-m", "landed"]);
    ok(&fx, &["undo"]);

    fx.write("a.txt", "a different direction\n");
    ok(&fx, &["-m", "elsewhere"]);

    let out = ff(&fx, &["redo", "--json"]);
    assert!(!out.status.success(), "the log forked");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "op/nothing-to-redo");
    assert_eq!(
        body(&fx),
        "a different direction\n",
        "and nothing was stepped over"
    );

    // Nothing was discarded either: the abandoned operation still resolves,
    // and landing on it by id still works.
    let out = ff(&fx, &["op", "log", "--captures", "--json"]);
    assert!(out.status.success());
}

/// With nothing recorded, undo says so rather than doing something.
#[test]
fn undo_on_a_fresh_log_says_there_is_nothing_to_do() {
    let fx = base();
    let out = ff(&fx, &["undo", "--json"]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let id = v["error"]["id"].as_str().expect("id");
    assert!(
        id == "undo/nothing" || id == "op/floor",
        "nothing to step back over: {v}"
    );

    // And redo has nothing to reverse.
    let out = ff(&fx, &["redo", "--json"]);
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "op/nothing-to-redo");
}

/// Undo takes no argument and no `--force`: naming one operation is
/// `ff op restore`, where doing so is already a deliberate act.
#[test]
fn undo_takes_no_argument() {
    let fx = base();
    fx.write("a.txt", "work\n");
    ok(&fx, &["commit", "-m", "landed"]);

    for args in [vec!["undo", "kqzm"], vec!["undo", "--force"]] {
        let out = ff(&fx, &args);
        assert_eq!(out.status.code(), Some(2), "ff {args:?} must not parse");
    }
    // The long form takes both.
    assert!(
        ff(&fx, &["op", "restore", "@^", "--force"])
            .status
            .success(),
        "ff op restore takes an operation and --force"
    );
}
