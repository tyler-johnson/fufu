//! `ff history` end to end — and, more to the point, `ff history` held to
//! `ff undo`.
//!
//! The view and the verb are the same claim written twice: one says where you
//! can go, the other goes there. That is exactly the shape that drifts, so
//! the property worth a test is the agreement rather than the rendering —
//! **for every row listed below `@`, that many presses of `ff undo` land on
//! that row's id**. Nothing else here would catch the two coming apart.

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
        .env_remove("FF_SESSION")
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

fn ok(fx: &Fixture, args: &[&str]) -> String {
    let out = ff(fx, args);
    assert!(out.status.success(), "ff {args:?} failed: {}", stderr(&out));
    stdout(&out)
}

/// A repository with commits, a run of captures, and one operation of every
/// shape `ff history` has to collapse.
fn with_moves() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "History Tester");
    fx.set_config("user.email", "history@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    fx.write("a.txt", "the parser\n");
    assert!(ff(&fx, &["commit", "-m", "the parser"]).status.success());
    // Three adjacent captures with no verb between them: one run, and so one
    // row and one undo.
    for i in 1..=3 {
        fx.write("b.txt", &format!("draft {i}\n"));
        assert!(ff(&fx, &[]).status.success(), "bare ff captures");
    }
    assert!(ff(&fx, &["commit", "-m", "help pages"]).status.success());
    fx
}

/// The rows, as `(distance, id)`, in the order they print.
fn rows(fx: &Fixture) -> Vec<(i64, String)> {
    let body = ok(fx, &["history", "-n", "0", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    value["data"]["steps"]
        .as_array()
        .expect("steps array")
        .iter()
        .map(|s| {
            (
                s["distance"].as_i64().expect("distance"),
                s["id"].as_str().expect("id").to_string(),
            )
        })
        .collect()
}

/// The id `@` sits on right now.
fn standing(fx: &Fixture) -> String {
    rows(fx)
        .into_iter()
        .find(|(d, _)| *d == 0)
        .expect("exactly one row is now")
        .1
}

/// The property the whole verb exists to keep. Checked one press at a time,
/// which is the same claim as "N presses reach row N" and costs one pass
/// instead of N restores.
#[test]
fn every_row_below_now_is_that_many_undos_away() {
    let fx = with_moves();
    let listed: Vec<(i64, String)> = rows(&fx).into_iter().filter(|(d, _)| *d > 0).collect();
    assert!(listed.len() >= 3, "enough steps to walk: {listed:?}");

    for (distance, id) in &listed {
        let out = ff(&fx, &["undo"]);
        assert!(
            out.status.success(),
            "undo #{distance} failed: {}",
            stderr(&out)
        );
        assert_eq!(
            &standing(&fx),
            id,
            "undo #{distance} lands on the row ff history listed at that distance"
        );
    }
}

/// A run of captures is one row, because it is one undo — and the row says
/// how many it collapsed, since a keystroke that moved three operations must
/// not have to be inferred.
#[test]
fn a_run_of_captures_is_one_row_and_says_how_many() {
    let fx = with_moves();
    let body = ok(&fx, &["history", "-n", "0", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    let steps = value["data"]["steps"].as_array().expect("steps array");

    let collapsed: Vec<i64> = steps
        .iter()
        .filter_map(|s| s["collapsed"].as_i64())
        .collect();
    assert!(
        collapsed.iter().any(|n| *n >= 3),
        "the three-capture run is one row carrying its count: {collapsed:?}"
    );

    // And the whole log is longer than the list of moves over it — which is
    // the reason this view exists rather than `ff op log` answering it.
    let ops = ok(&fx, &["op", "log", "--captures", "-n", "0", "--json"]);
    let ops: serde_json::Value = serde_json::from_str(&ops).expect("valid json");
    assert!(
        ops["data"]["ops"].as_array().expect("ops").len() > steps.len(),
        "more operations than moves"
    );
}

/// Redo is a path, not a single step, and it is spent by taking it.
#[test]
fn the_redo_path_is_above_now_and_is_spent_by_walking_it() {
    let fx = with_moves();
    assert!(
        !rows(&fx).iter().any(|(d, _)| *d < 0),
        "nothing to redo before anything is undone"
    );

    let was_standing = standing(&fx);
    assert!(ff(&fx, &["undo"]).status.success());

    let after_undo = rows(&fx);
    assert_eq!(
        after_undo.iter().find(|(d, _)| *d == -1).map(|(_, id)| id),
        Some(&was_standing),
        "where undo came from is one press of redo above now: {after_undo:?}"
    );

    assert!(ff(&fx, &["redo"]).status.success());
    let after_redo = rows(&fx);
    assert_eq!(standing(&fx), was_standing, "redo went back");
    assert!(
        !after_redo.iter().any(|(d, _)| *d < 0),
        "the redo row is gone once it has been taken: {after_redo:?}"
    );
}

/// The envelope names the verb, and the payload says both things a machine
/// needs: the rows, and whether the oldest one is the floor or merely the
/// last one `-n` allowed.
#[test]
fn the_machine_surface_carries_the_rows_and_the_floor() {
    let fx = with_moves();

    let body = ok(&fx, &["history", "-n", "0", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(value["cmd"], "history");
    assert_eq!(value["data"]["floor"], true, "unbounded reaches the floor");

    let now = value["data"]["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .find(|s| s["distance"] == 0)
        .expect("a now row");
    assert_eq!(now["landing"], "now");
    for field in ["id", "short_id", "kind", "summary", "time"] {
        assert!(!now[field].is_null(), "{field} is on the row: {now}");
    }

    // Bounded, and honest about it: one step shown, and no claim of a floor.
    let body = ok(&fx, &["history", "-n", "1", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(value["data"]["floor"], false, "-n 1 stopped short");
    let back = value["data"]["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .filter(|s| s["distance"].as_i64().expect("distance") > 0)
        .count();
    assert_eq!(back, 1, "one step back is one row back");
}

/// The human rendering: a marker column that says how many presses, and the
/// floor named rather than left as a row that simply stops.
#[test]
fn the_rows_say_how_many_presses_and_where_the_floor_is() {
    let fx = with_moves();
    let body = ok(&fx, &["history", "-n", "0"]);
    assert!(body.contains("@ "), "now is marked: {body}");
    assert!(
        body.contains("↓1"),
        "the first step back is numbered: {body}"
    );
    assert!(body.contains("(the floor)"), "the floor is named: {body}");
}
