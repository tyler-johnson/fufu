//! The park is the open commit, as the CLI says it: a dirty switch leaves
//! nothing on `git stash list`, a conflicting arrival holds the branch and
//! exits 3, `ff status` names the hold, `ff resolve` lays the change into
//! the working copy with markers, a legacy park folds with a line saying so,
//! and `ff doctor` lists parks by their open refs.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;
use ff_testsupport::{Fixture, legacy_park};

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

fn ok(out: Output) -> Output {
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

/// `main` and `other` over one file, with an identity.
fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Park Tester");
    fx.set_config("user.email", "park@test.test");
    fx.write("a.txt", "base\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    fx
}

/// Park a conflicting edit on `main` and move `main`'s tip under it: the
/// next `ff switch main` arrives held.
fn park_and_move_under(fx: &Fixture) {
    fx.write("a.txt", "wip\n");
    ok(ff(fx, &["switch", "other"]));
    fx.write("a.txt", "conflicting\n");
    ok(ff(fx, &["commit", "-m", "advance"]));
    ok(ff(fx, &["git", "update-ref", "refs/heads/main", "other"]));
}

#[test]
fn a_dirty_switch_leaves_nothing_on_the_stash_list() {
    let fx = repo();
    fx.write("a.txt", "wip\n");
    let text = stdout(&ok(ff(&fx, &["switch", "other"])));
    assert!(text.contains("parked the open change on main ("), "{text}");
    assert!(fx.git(&["stash", "list"]).is_empty());
    let open = fx
        .git(&["rev-parse", "refs/fufu/open/main"])
        .trim()
        .to_string();
    assert!(
        fx.git(&["log", "--all", "--oneline"]).contains(&open[..7]),
        "git log --all shows the open commit"
    );
    let back = stdout(&ok(ff(&fx, &["switch", "main"])));
    assert!(
        back.contains("resumed the parked change (1 file(s))"),
        "{back}"
    );
}

#[test]
fn a_held_arrival_exits_3_and_status_names_it() {
    let fx = repo();
    park_and_move_under(&fx);

    let out = ff(&fx, &["switch", "main"]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("switched to main"), "{text}");
    assert!(
        text.contains("the parked change does not apply on main's new tip:"),
        "{text}"
    );
    assert!(text.contains("  conflicts: a.txt"), "{text}");
    assert!(
        text.contains("held: ff resolve lays it into the open change with markers"),
        "{text}"
    );

    let status = stdout(&ok(ff(&fx, &["status"])));
    assert!(
        status.contains("held: ff switch conflicts at your open change in 1 file"),
        "{status}"
    );

    let v = json(&ok(ff(&fx, &["status", "--json"])));
    assert_eq!(v["data"]["held"]["verb"], "switch");
    assert_eq!(v["data"]["held"]["paths"][0], "a.txt");
}

#[test]
fn a_held_arrival_in_json_exits_3() {
    let fx = repo();
    park_and_move_under(&fx);
    let out = ff(&fx, &["--json", "switch", "main"]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout(&out));
    let v = json(&out);
    assert_eq!(v["data"]["switch"]["arrival"]["state"], "held");
    assert_eq!(v["data"]["switch"]["arrival"]["paths"][0], "a.txt");
    assert!(v["data"]["switch"]["arrival"]["open"].is_string());
}

#[test]
fn resolve_lays_the_markers_into_the_open_change() {
    let fx = repo();
    park_and_move_under(&fx);
    let out = ff(&fx, &["switch", "main"]);
    assert_eq!(out.status.code(), Some(3));

    let text = stdout(&ok(ff(&fx, &["resolve"])));
    assert!(
        text.contains("laid the parked change over main with conflict markers in 1 file(s):"),
        "{text}"
    );
    assert!(text.contains("  a.txt"), "{text}");
    assert!(
        text.contains("fix the markers; the open change is the resolution"),
        "{text}"
    );
    let content = std::fs::read_to_string(fx.path().join("a.txt")).unwrap();
    assert!(content.contains("<<<<<<<"), "{content}");

    // Fix, then close: the change lands with its original id.
    let id_before = json(&ok(ff(&fx, &["log", "--json"])))["data"]["open"]["change_id"]
        .as_str()
        .map(str::to_string);
    fx.write("a.txt", "resolved\n");
    ok(ff(&fx, &["commit", "-m", "the change"]));
    let landed = json(&ok(ff(&fx, &["log", "--json", "-n", "1"])));
    let landed_id = landed["data"]["commits"][0]["change_id"]
        .as_str()
        .map(str::to_string);
    assert!(landed_id.is_some());
    assert_eq!(landed_id, id_before, "{landed}");
}

#[test]
fn resolve_abandon_drops_the_held_arrival_and_names_the_commit() {
    let fx = repo();
    park_and_move_under(&fx);
    let _ = ff(&fx, &["switch", "main"]);
    let text = stdout(&ok(ff(&fx, &["resolve", "--abandon"])));
    assert!(
        text.contains("dropped the held arrival on main; the parked change stays at"),
        "{text}"
    );
    let status = stdout(&ok(ff(&fx, &["status"])));
    assert!(!status.contains("held:"), "{status}");
}

#[test]
fn a_landed_park_says_so() {
    let fx = repo();
    fx.write("a.txt", "wip\n");
    ok(ff(&fx, &["switch", "other"]));
    fx.write("a.txt", "wip\n");
    ok(ff(&fx, &["commit", "-m", "the same edit"]));
    ok(ff(&fx, &["git", "update-ref", "refs/heads/main", "other"]));
    let text = stdout(&ok(ff(&fx, &["switch", "main"])));
    assert!(
        text.contains("the parked change is already in main; nothing to resume"),
        "{text}"
    );
}

#[test]
fn a_legacy_park_folds_with_a_line_and_the_doctor_lists_it_first() {
    let fx = repo();
    fx.write("a.txt", "wip\n");
    let stash = legacy_park(&fx, "main");
    ok(ff(&fx, &["switch", "other"]));

    let doctor = stdout(&ff(&fx, &["doctor"]));
    assert!(
        doctor.contains("legacy parks awaiting fold: main — ff switch <branch> folds each"),
        "{doctor}"
    );

    let text = stdout(&ok(ff(&fx, &["switch", "main"])));
    assert!(
        text.contains("resumed the parked change (1 file(s))"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "folded its stash entry ({}) into the open commit",
            &stash[..8]
        )),
        "{text}"
    );
    assert!(fx.git(&["stash", "list"]).is_empty());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "wip\n"
    );
}

#[test]
fn the_doctor_lists_parks_by_their_open_refs() {
    let fx = repo();
    fx.write("a.txt", "wip\n");
    ok(ff(&fx, &["switch", "other"]));
    let doctor = stdout(&ff(&fx, &["doctor"]));
    assert!(doctor.contains("tree memory held for: main"), "{doctor}");
    assert!(!doctor.contains("legacy parks"), "{doctor}");

    // The branch underfoot's open change is not a park.
    fx.write("a.txt", "other's wip\n");
    let doctor = stdout(&ff(&fx, &["doctor"]));
    let row = doctor
        .lines()
        .find(|l| l.contains("tree memory held for:"))
        .unwrap();
    assert_eq!(
        row.trim_end().rsplit("held for: ").next(),
        Some("main"),
        "{doctor}"
    );
}
