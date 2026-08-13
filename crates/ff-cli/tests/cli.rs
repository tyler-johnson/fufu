//! CLI conventions: JSON shapes, exit codes, human output basics.
//! Runs the real `ff` binary against hermetic fixtures.

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
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

#[test]
fn status_json_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["head"]["state"], "branch");
    assert_eq!(v["head"]["name"], "main");
    assert_eq!(v["head"]["ref"], "refs/heads/main");
    assert_eq!(v["operation"], serde_json::Value::Null);
    assert_eq!(v["upstream"], serde_json::Value::Null);
    assert_eq!(v["unstaged"][0]["path"], "a.txt");
    assert_eq!(v["unstaged"][0]["kind"], "modified");
    assert_eq!(v["untracked"][0], "new.txt");
    assert_eq!(v["staged"].as_array().unwrap().len(), 0);
    assert_eq!(v["conflicts"].as_array().unwrap().len(), 0);
}

#[test]
fn status_json_is_stable_bytes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let a = ff(&fx, &["status", "--json"]);
    let b = ff(&fx, &["status", "--json"]);
    assert_eq!(a.stdout, b.stdout, "identical bytes run to run");
}

#[test]
fn status_human_header() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("on main\n"), "header: {text:?}");
    assert!(text.contains("clean"), "clean tree: {text:?}");
}

#[test]
fn status_human_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("on main (no commits yet)"));
}

#[test]
fn log_json_envelope_and_limit() {
    let fx = Fixture::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json", "-n", "2"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let commits = v["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "c3");
    for key in [
        "id",
        "short_id",
        "subject",
        "author_name",
        "author_email",
        "time",
    ] {
        assert!(!commits[0][key].is_null(), "missing key {key}");
    }
}

#[test]
fn log_defaults_to_25() {
    let fx = Fixture::new();
    for i in 0..30 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["commits"].as_array().unwrap().len(), 25);
    let out = ff(&fx, &["log", "--json", "-n", "0"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["commits"].as_array().unwrap().len(),
        30,
        "-n 0 is unlimited"
    );
}

#[test]
fn log_unborn_is_empty_success() {
    let fx = Fixture::new();
    let out = ff(&fx, &["log", "--commits", "--json"]);
    assert!(out.status.success(), "unborn log exits 0");
    assert_eq!(stdout(&out), "{\"commits\":[]}\n");
    // The default view still carries the open change, with null fields.
    let out = ff(&fx, &["log", "--json"]);
    assert_eq!(
        stdout(&out),
        "{\"commits\":[],\"open\":{\"branch\":\"main\",\"id\":null,\"id_letters\":null,\
         \"base\":null,\"subject\":null,\"time\":null,\"clean\":true}}\n"
    );
    let human = ff(&fx, &["log", "--commits"]);
    assert!(human.status.success());
    assert_eq!(stdout(&human), "", "unborn --commits human prints nothing");
    // The default human view shows the @ row alone: no commits, no ● rows.
    let human = ff(&fx, &["log"]);
    let text = stdout(&human);
    assert!(text.starts_with("@"), "unborn @ row: {text:?}");
    assert!(text.contains("(no commits yet)"), "{text:?}");
    assert!(!text.contains('●'), "no commit rows: {text:?}");
}

#[test]
fn bare_repo_status_errors_log_works() {
    let fx = Fixture::new_bare();
    let status = ff(&fx, &["status"]);
    assert_eq!(status.status.code(), Some(1));
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(stderr.starts_with("ff: "), "error convention: {stderr:?}");
    let log = ff(&fx, &["log", "--json"]);
    assert!(log.status.success(), "bare log works");
}

#[test]
fn outside_repo_is_runtime_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = ff_at(dir.path(), &["status"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("ff: "));
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fixture::new();
    let unknown_flag = ff(&fx, &["status", "--nope"]);
    assert_eq!(unknown_flag.status.code(), Some(2));
    // Bare-ff args conflict with subcommands: `ff -m x status` is nonsense.
    let mixed = ff(&fx, &["-m", "msg", "status"]);
    assert_eq!(mixed.status.code(), Some(2));
    let bad_count = ff(&fx, &["log", "-n", "many"]);
    assert_eq!(bad_count.status.code(), Some(2));
}

#[test]
fn bare_ff_snapshots_and_noops() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(
        text.starts_with("snapshot ") && text.contains(" on main\n"),
        "created line: {text:?}"
    );
    // The top of the snapshot chain follows after a blank line.
    assert!(text.contains("\n\n"), "blank separator: {text:?}");
    assert!(
        text.contains("manual"),
        "confirmation shows the snapshot: {text:?}"
    );
    // Rows lead with the letters-spelled snapshot id.
    let confirmation = text.split("\n\n").nth(1).unwrap();
    assert!(
        confirmation.lines().all(|line| {
            let token = line.split_whitespace().next().unwrap_or("");
            token.len() >= 4 && token.chars().all(|c| ('k'..='z').contains(&c))
        }),
        "letters ids lead the rows: {confirmation:?}"
    );

    let chain = fx.git(&["rev-parse", "--verify", "refs/fufu/snap/main"]);
    assert!(!chain.trim().is_empty());

    let again = ff(&fx, &[]);
    assert_eq!(
        stdout(&again),
        "no changes since the last snapshot on main\n"
    );
}

#[test]
fn bare_ff_json_shapes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &["--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["outcome"], "created");
    assert_eq!(v["branch"], "main");
    assert!(
        v["id"]
            .as_str()
            .unwrap()
            .starts_with(v["short_id"].as_str().unwrap())
    );

    let again = ff(&fx, &["--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&again)).unwrap();
    assert_eq!(v["outcome"], "noop");
    assert_eq!(v["branch"], "main");
}

#[test]
fn bare_ff_message_becomes_subject() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["-m", "before the refactor"]);
    assert!(out.status.success());
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "manual: before the refactor");
}

#[test]
fn status_and_log_capture_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: ff status");

    fx.write("a.txt", "more dirt\n");
    let out = ff(&fx, &["log"]);
    assert!(out.status.success());
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: ff log");
}

#[test]
fn log_default_is_change_centric() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "two\n");
    fx.commit("landed");
    fx.write("a.txt", "three\n");
    assert!(ff(&fx, &[]).status.success());

    // Human view: the @ row leads, ● commit rows follow, subjects on │ rails.
    let out = ff(&fx, &["log"]);
    let text = stdout(&out);
    assert!(text.starts_with("@  "), "@ row leads: {text:?}");
    assert!(text.contains("\n●  "), "commit rows: {text:?}");
    assert!(text.contains("\n│  "), "subject rails: {text:?}");
    assert!(
        text.contains("(no description)"),
        "open change without a pending description: {text:?}"
    );
    assert!(
        text.contains("│  landed") && text.contains("│  init"),
        "commit subjects present: {text:?}"
    );

    // JSON: commits key preserved, open object present, timeline gone.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["commits"].is_array());
    assert!(v.get("timeline").is_none(), "timeline key retired");
    assert_eq!(v["open"]["branch"], "main");
    assert!(v["open"]["id"].is_string(), "chain tip present");
    let letters = v["open"]["id_letters"].as_str().unwrap();
    assert!(
        letters.chars().all(|c| ('k'..='z').contains(&c)),
        "letters spelling at the JSON edge: {letters:?}"
    );
    assert_eq!(v["open"]["clean"], false, "uncaptured-free but dirty tree");

    // --commits --json keeps the exact Phase 0 envelope.
    let out = ff(&fx, &["log", "--commits", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["commits"].is_array());
    assert!(v.get("open").is_none(), "no open key in commits view");
    assert!(
        v.get("timeline").is_none(),
        "no timeline key in commits view"
    );
}

/// The @ row states: pending description, (clean), dirty, described.
#[test]
fn log_at_row_states() {
    let fx = Fixture::new();
    fx.set_config("user.name", "At Row");
    fx.set_config("user.email", "at@row.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Clean tree, no chain: (clean) marker, no description.
    let text = stdout(&ff(&fx, &["log"]));
    let mut lines = text.lines();
    let at = lines.next().unwrap();
    assert!(at.starts_with("@  ") && at.ends_with("(clean)"), "{at:?}");
    assert_eq!(lines.next().unwrap(), "│  (no description)");

    // Dirty tree: ff log's own pre-capture becomes the tip — not clean.
    fx.write("a.txt", "dirty\n");
    let text = stdout(&ff(&fx, &["log"]));
    let at = lines_first(&text);
    assert!(!at.contains("(clean)"), "{at:?}");

    // A pending description owns the subject line.
    assert!(
        ff(&fx, &["describe", "-m", "work in progress"])
            .status
            .success()
    );
    let text = stdout(&ff(&fx, &["log"]));
    assert_eq!(text.lines().nth(1).unwrap(), "│  work in progress");

    // Closing the change lands the captured state: clean again.
    assert!(ff(&fx, &["commit", "-m", "landed"]).status.success());
    let text = stdout(&ff(&fx, &["log"]));
    assert!(lines_first(&text).ends_with("(clean)"), "{text:?}");
}

fn lines_first(text: &str) -> &str {
    text.lines().next().unwrap()
}

/// ● rows carry the segment tip (newest snapshot based on that commit) in
/// the letters column, or leave it blank when no snapshot ever sat there.
#[test]
fn log_segment_tips_fill_and_blank() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let bare = fx.commit("no snapshots here");
    fx.write("a.txt", "b\n");
    let snapped = fx.commit("snapshots land here");
    fx.write("a.txt", "dirty\n");
    assert!(ff(&fx, &[]).status.success());

    let text = stdout(&ff(&fx, &["log"]));
    let row_of = |sha: &str| {
        text.lines()
            .find(|line| line.starts_with('●') && line.contains(&sha[..8]))
            .unwrap_or_else(|| panic!("no ● row for {sha}: {text:?}"))
            .to_string()
    };
    let snapped_row = row_of(&snapped);
    let tokens: Vec<&str> = snapped_row.split_whitespace().collect();
    assert!(
        tokens[1].len() == 8 && tokens[1].chars().all(|c| ('k'..='z').contains(&c)),
        "segment tip in the letters column: {snapped_row:?}"
    );
    let bare_row = row_of(&bare);
    let tokens: Vec<&str> = bare_row.split_whitespace().collect();
    assert!(
        tokens[1].chars().all(|c| c.is_ascii_hexdigit()),
        "blank letters column jumps straight to the sha: {bare_row:?}"
    );
}

#[test]
fn evolog_lists_snapshots_and_json() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Empty chain: a friendly line, exit 0.
    let out = ff(&fx, &["evolog"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "no snapshots on main yet\n");

    fx.write("a.txt", "one\n");
    assert!(ff(&fx, &["-m", "first"]).status.success());
    fx.write("a.txt", "two\n");
    assert!(ff(&fx, &["-m", "second"]).status.success());

    let out = ff(&fx, &["evolog"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "snapshot rows only: {text:?}");
    assert!(lines[0].contains("second") && lines[1].contains("first"));
    for line in &lines {
        let token = line.split_whitespace().next().unwrap();
        assert_eq!(token.len(), 8, "letters8 id column: {line:?}");
        assert!(
            token.chars().all(|c| ('k'..='z').contains(&c)),
            "letters alphabet: {token:?}"
        );
    }

    let out = ff(&fx, &["evolog", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let snaps = v["snapshots"].as_array().unwrap();
    assert_eq!(snaps.len(), 2);
    assert!(
        snaps[0]["id"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "JSON ids stay hex"
    );
}

/// A letters id copied from evolog output round-trips into `ff restore --at`.
#[test]
fn restore_accepts_letters_id_from_evolog() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &["-m", "keep this"]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["evolog"]);
    let text = stdout(&out);
    let letters = text
        .lines()
        .find(|line| line.contains("keep this"))
        .and_then(|line| line.split_whitespace().next())
        .expect("letters id on the manual row")
        .to_string();

    let out = ff(&fx, &["restore", "--all", "--at", &letters]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );
}

#[test]
fn restore_requires_paths_or_all() {
    let fx = Fixture::new();
    let out = ff(&fx, &["restore"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "paths XOR --all is a usage error"
    );
    let out = ff(&fx, &["restore", "--all", "some/path"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn restore_round_trip_with_undo_hint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["restore", "--all"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.starts_with("restored to "), "header: {text:?}");
    assert!(text.contains("restored  a.txt"), "file list: {text:?}");
    assert!(
        text.trim_end().ends_with("undo: ff restore --all"),
        "undo hint: {text:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );

    // The undo hint works: restore --all again returns to the diverged state
    // (captured by restore's own mandatory pre-snapshot).
    let out = ff(&fx, &["restore", "--all"]);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "diverged\n"
    );
}

#[test]
fn restore_json_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["restore", "--all", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["target"]["id"].is_string());
    assert_eq!(v["restored"][0], "a.txt");
    assert_eq!(v["undo"], "ff restore --all");
    assert!(
        v["pre_snapshot"].is_string(),
        "pre-restore snapshot recorded"
    );
}

#[test]
fn trim_reports_and_dry_runs() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "fresh\n");
    assert!(ff(&fx, &[]).status.success());

    let out = ff(&fx, &["trim"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("main: nothing to drop"),
        "fresh chains report kept counts: {text:?}"
    );

    let out = ff(&fx, &["trim", "--dry-run", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["chains"][0]["branch"], "main");
    assert_eq!(v["chains"][0]["dropped"], 0);
}

#[test]
fn merge_conflict_renders_sections() {
    let fx = Fixture::new();
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    fx.commit("ours");
    let merge = fx.try_git(&["merge", "other"]);
    assert!(!merge.status.success());

    let out = ff(&fx, &["status"]);
    let text = stdout(&out);
    assert!(text.starts_with("on main · merging\n"), "header: {text:?}");
    assert!(
        text.contains("conflicts:\n  conflict.txt\n"),
        "body: {text:?}"
    );
}
