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

/// `ff` with color forced on despite the captured (non-TTY) stdout —
/// anstream honors `CLICOLOR_FORCE`, which is the only way a test can reach
/// the styling paths at all.
fn ff_colored(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("CLICOLOR_FORCE", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// The letters spelling of a hex id's first 8 digits: the alphabet is the
/// k–z run, so an id can never be misread as a commit sha.
fn letters8(hex: &str) -> String {
    const ALPHABET: &[u8; 16] = b"zyxwvutsrqponmlk";
    hex[..8]
        .chars()
        .map(|c| ALPHABET[c.to_digit(16).unwrap() as usize] as char)
        .collect()
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
    let d = &v["data"];
    assert_eq!(d["head"]["state"], "branch");
    assert_eq!(d["head"]["name"], "main");
    assert_eq!(d["head"]["ref"], "refs/heads/main");
    assert_eq!(d["operation"], serde_json::Value::Null);
    assert_eq!(d["upstream"], serde_json::Value::Null);
    // Old keys are gone
    assert_eq!(d["staged"], serde_json::Value::Null);
    assert_eq!(d["unstaged"], serde_json::Value::Null);
    assert_eq!(d["untracked"], serde_json::Value::Null);
    // New shape: changes array with modified + added (untracked = ordinary addition)
    assert!(d["changes"].is_array(), "changes is array");
    let changes = d["changes"].as_array().unwrap();
    assert!(changes.len() >= 2, "at least modified and added entries");
    let kinds: std::collections::HashSet<_> = changes
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains("modified"), "modified entry present");
    assert!(kinds.contains("added"), "added entry present");
    assert_eq!(d["conflicts"].as_array().unwrap().len(), 0);
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
    assert!(
        text.contains("no changes"),
        "clean tree shows no changes: {text:?}"
    );
}

#[test]
fn status_human_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("on main (no commits yet)"));
    // No commit row (●) when unborn
    assert!(!text.contains("●"), "no parent row when unborn: {text:?}");
}

#[test]
fn status_human_shows_stat_rows() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\nworld\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked content\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("a.txt"), "modified file visible: {text:?}");
    assert!(text.contains("new.txt"), "untracked file visible: {text:?}");
    assert!(text.contains("+"), "insertion count present: {text:?}");
    assert!(text.contains("2 files"), "summary row: {text:?}");
}

#[test]
fn status_human_has_no_staging_words() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("staged"), "no 'staged': {text:?}");
    assert!(!text.contains("unstaged"), "no 'unstaged': {text:?}");
    assert!(!text.contains("untracked"), "no 'untracked': {text:?}");
}

#[test]
fn status_human_parent_row() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("first");
    fx.write("a.txt", "two\n");
    fx.commit("second");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("●"), "parent row bullet present: {text:?}");
    assert!(text.contains("second"), "parent subject visible: {text:?}");
}

#[test]
fn status_json_parent_null_when_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["parent"], serde_json::Value::Null);
}

#[test]
fn status_json_reports_foreign_motion() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    // Bootstrap the journal so reconcile has a baseline
    let _ = ff(&fx, &["status"]);
    // Move HEAD with raw git so reconcile detects foreign motion
    fx.git(&["commit", "--amend", "--no-edit"]);
    // First ff status after the amend: reconcile absorbs AND reports the foreign change
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let foreign = &v["data"]["foreign"];
    assert!(foreign.is_array(), "foreign is array: {foreign}");
    assert!(
        !foreign.as_array().unwrap().is_empty(),
        "foreign is non-empty"
    );
    let first = &foreign[0];
    assert!(first.get("ref").is_some(), "has ref key");
    assert!(first.get("old").is_some(), "has old key");
    assert!(first.get("new").is_some(), "has new key");
}

#[test]
fn status_json_foreign_is_null_when_clean() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["foreign"], serde_json::Value::Null);
}

#[test]
fn status_json_keys_are_unchanged() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("b.txt", "modified\n");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let d = &v["data"];
    // Every pre-existing key must be present (non-null for non-optional fields)
    for key in [
        "head",
        "changes",
        "insertions",
        "deletions",
        "open",
        "conflicts",
    ] {
        assert!(!d[key].is_null(), "key {} is non-null", key);
    }
    // Optional keys exist (may be null)
    for key in ["operation", "upstream", "parent", "foreign"] {
        assert!(d.get(key).is_some(), "key {} exists", key);
    }
    // open sub-keys
    let open = &d["open"];
    for key in ["id", "id_letters", "pending", "subject", "clean"] {
        assert!(open.get(key).is_some(), "open.{} exists", key);
    }
}

#[test]
fn status_human_output_is_unchanged() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    // Two-row shape: header + open change row (plus diffstat)
    assert!(text.starts_with("on main\n"), "header line: {text:?}");
    // Diffstat line for the modified file
    assert!(
        text.contains("a.txt"),
        "modified file in diffstat: {text:?}"
    );
    assert!(text.contains("1 file"), "summary row: {text:?}");
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
    let commits = v["data"]["commits"].as_array().unwrap();
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
    assert_eq!(v["data"]["commits"].as_array().unwrap().len(), 25);
    let out = ff(&fx, &["log", "--json", "-n", "0"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["data"]["commits"].as_array().unwrap().len(),
        30,
        "-n 0 is unlimited"
    );
}

#[test]
fn log_unborn_is_empty_success() {
    let fx = Fixture::new();
    let out = ff(&fx, &["log", "--commits", "--json"]);
    assert!(out.status.success(), "unborn log exits 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["data"]["commits"].as_array().unwrap().is_empty());
    // The default view still carries the open change, with null fields.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert!(d["commits"].as_array().unwrap().is_empty());
    assert_eq!(d["open"]["branch"], "main");
    assert!(d["open"]["id"].is_null());
    assert!(d["open"]["clean"].is_boolean());
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
fn errors_carry_exactly_one_ff_prefix() {
    // The CLI owns the `ff: ` prefix (main.rs); engine messages must not
    // embed their own, or errors print as `ff: ff: ...`.
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["start", "@"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("ff: "), "error convention: {stderr:?}");
    assert!(!stderr.contains("ff: ff: "), "doubled prefix: {stderr:?}");
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
    // `-m` is retired outright: the old snapshot message has no home on any
    // command line, so it exits 2 whether or not a verb follows it.
    let mixed = ff(&fx, &["-m", "msg", "status"]);
    assert_eq!(mixed.status.code(), Some(2));
    let retired = ff(&fx, &["-m", "msg"]);
    assert_eq!(retired.status.code(), Some(2));
    let bad_count = ff(&fx, &["log", "-n", "many"]);
    assert_eq!(bad_count.status.code(), Some(2));
}

#[test]
fn bare_ff_draws_the_map() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "m1\n");
    fx.commit("main one");
    fx.write("a.txt", "m2\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.first().is_some_and(|line| line.starts_with('@')),
        "the open change leads the map: {text:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("feature")),
        "the other branch's name is on the map: {text:?}"
    );
    // The old confirmation line went away with the verb.
    assert!(
        !lines.iter().any(|line| line.contains("snapshot ")),
        "no snapshot confirmation: {text:?}"
    );
    // Capture did not go anywhere: bare ff still writes the chain first.
    let chain = fx.git(&["rev-parse", "--verify", "refs/fufu/snap/main"]);
    assert!(!chain.trim().is_empty());

    // The map is a view: a second run over the same tree draws the same
    // skeleton, and there is no "no changes since the last snapshot" line.
    let again = ff(&fx, &[]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        stdout(&again)
            .lines()
            .next()
            .is_some_and(|line| line.starts_with('@')),
        "still the map on the second run"
    );
}

#[test]
fn bare_ff_json_is_the_map() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "m1\n");
    fx.commit("main one");
    fx.write("a.txt", "m2\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &["--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "map");
    let rows = v["data"]["rows"].as_array().expect("rows array");
    assert!(!rows.is_empty(), "the map has rows");
    assert!(v["data"]["truncated"].is_boolean());
    assert_eq!(rows[0]["node"]["kind"], "open");
    assert!(
        rows.iter().any(|row| row["node"]["kind"] == "commit"),
        "a commit row is on the map"
    );
}

#[test]
fn the_snapshot_verb_is_retired() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["-m", "before the refactor"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("-m is gone"),
        "stderr names the removal: {stderr:?}"
    );

    // The machine surface carries the same coded refusal.
    let out = ff(&fx, &["-m", "x", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");
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
    let d = &v["data"];
    assert!(d["commits"].is_array());
    assert!(d.get("timeline").is_none(), "timeline key retired");
    assert_eq!(d["open"]["branch"], "main");
    assert!(d["open"]["id"].is_string(), "chain tip present");
    let letters = d["open"]["id_letters"].as_str().unwrap();
    assert!(
        letters.chars().all(|c| ('k'..='z').contains(&c)),
        "letters spelling at the JSON edge: {letters:?}"
    );
    assert_eq!(d["open"]["clean"], false, "uncaptured-free but dirty tree");
    assert!(
        d["open"]["pending"].is_null(),
        "no identity configured in the fixture"
    );

    // --commits --json keeps the exact Phase 0 envelope.
    let out = ff(&fx, &["log", "--commits", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert!(d["commits"].is_array());
    assert!(d.get("open").is_none(), "no open key in commits view");
    assert!(
        d.get("timeline").is_none(),
        "no timeline key in commits view"
    );
}

/// The @ row states: clean undescribed collapses to "no changes", dirty shows
/// letters + pending sha, describe changes the pending sha, close returns to
/// "no changes", clean+described shows a pending empty commit whose letters
/// match the ● anchor row.
#[test]
fn log_at_row_states() {
    let fx = Fixture::new();
    fx.set_config("user.name", "At Row");
    fx.set_config("user.email", "at@row.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Clean, undescribed: collapsed "no changes" line.
    let text = stdout(&ff(&fx, &["log"]));
    let lines = text.lines().collect::<Vec<&str>>();
    assert_eq!(lines[0], "@  no changes", "{text:?}");
    assert_eq!(lines[1], "│  (no description)", "{text:?}");

    // Dirty tree: ff log's own pre-capture becomes the tip.
    fx.write("a.txt", "dirty\n");
    let text = stdout(&ff(&fx, &["log"]));
    let at_line = text.lines().next().unwrap();
    let tokens: Vec<&str> = at_line.split_whitespace().collect();
    assert_eq!(tokens[0], "@");
    assert!(
        tokens[1].len() == 8 && tokens[1].chars().all(|c| ('k'..='z').contains(&c)),
        "tip letters: {at_line:?}"
    );
    assert!(
        tokens[2].len() == 8 && tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
        "pending sha: {at_line:?}"
    );
    let dirty_letters = tokens[1].to_string();
    let dirty_sha = tokens[2].to_string();

    // Describe while dirty: letters unchanged, pending sha changes.
    assert!(
        ff(&fx, &["describe", "-m", "work in progress"])
            .status
            .success()
    );
    let text = stdout(&ff(&fx, &["log"]));
    assert_eq!(text.lines().nth(1).unwrap(), "│  work in progress");
    let tokens: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(tokens[1], dirty_letters, "letters unchanged after describe");
    assert_ne!(tokens[2], dirty_sha, "pending sha changed after describe");

    // Close: back to "no changes".
    assert!(ff(&fx, &["commit", "-m", "landed"]).status.success());
    let text = stdout(&ff(&fx, &["log"]));
    assert_eq!(text.lines().next().unwrap(), "@  no changes");

    // Describe while clean: pending empty commit, letters match ● anchor row.
    assert!(ff(&fx, &["describe", "-m", "next up"]).status.success());
    let text = stdout(&ff(&fx, &["log"]));
    let tokens: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    assert!(
        tokens[1].len() == 8 && tokens[1].chars().all(|c| ('k'..='z').contains(&c)),
        "clean+described letters: {text:?}"
    );
    assert!(
        tokens[2].len() == 8 && tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
        "clean+described pending sha: {text:?}"
    );
    let clean_letters = tokens[1].to_string();
    // The first ● row should have the same letters (anchor duplication).
    let bullet_line = text
        .lines()
        .find(|l| l.starts_with('●'))
        .expect("a ● row exists");
    let bullet_tokens: Vec<&str> = bullet_line.split_whitespace().collect();
    assert_eq!(
        bullet_tokens[1], clean_letters,
        "● row shares letters with @ row (anchor)"
    );
}

/// Anchor rule: a commit earns a letters id in the ● row when the live chain
/// has a snapshot whose base is the commit's first parent AND whose tree
/// equals the commit's tree. Git-made roots and partial-stage commits have
/// no matching snapshot → blank letters column.
#[test]
fn log_segment_tips_fill_and_blank() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Segment User");
    fx.set_config("user.email", "segment@test");
    fx.write("a.txt", "a\n");
    let bare = fx.commit("no snapshots here");
    fx.write("a.txt", "b\n");
    assert!(ff(&fx, &["commit", "-m", "landed by ff"]).status.success());
    let landed = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.write("a.txt", "c\n");
    fx.write("other.txt", "x\n");
    assert!(ff(&fx, &[]).status.success());
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "partial"]);
    let partial = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let text = stdout(&ff(&fx, &["log"]));
    let row_of = |sha: &str| {
        text.lines()
            .find(|line| line.starts_with('●') && line.contains(&sha[..7]))
            .unwrap_or_else(|| panic!("no ● row for {sha}: {text:?}"))
            .to_string()
    };

    // landed's row: letters column is the pre-commit snapshot.
    let landed_row = row_of(&landed);
    let landed_tokens: Vec<&str> = landed_row.split_whitespace().collect();
    // Verify against evolog: find the snapshot with base == bare.
    let evolog_out = ff(&fx, &["evolog", "--json"]);
    let evolog: serde_json::Value = serde_json::from_str(&stdout(&evolog_out)).unwrap();
    let pre_snap = evolog["data"]["snapshots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            s["base"] == bare
                && s["subject"]
                    .as_str()
                    .map(|subj| subj.starts_with("pre: ff commit"))
                    .unwrap_or(false)
        })
        .expect("pre-commit snapshot exists");
    let expected_letters = letters8(pre_snap["id"].as_str().unwrap());
    assert_eq!(
        landed_tokens[1], expected_letters,
        "landed row letters match pre-commit snapshot"
    );

    // bare's row: no snapshot, so the letters column is the dotted filler and
    // the sha follows it. The filler is what makes the absence legible — eight
    // spaces read as indentation rather than as an empty column.
    let bare_row = row_of(&bare);
    let bare_tokens: Vec<&str> = bare_row.split_whitespace().collect();
    assert_eq!(
        bare_tokens[1], "—",
        "bare row's letters column is the empty-id filler: {bare_row:?}"
    );
    assert!(
        bare_tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
        "and the sha follows it: {bare_row:?}"
    );

    // partial's row: same, no snapshot answers it.
    let partial_row = row_of(&partial);
    let partial_tokens: Vec<&str> = partial_row.split_whitespace().collect();
    assert_eq!(
        partial_tokens[1], "—",
        "partial row's letters column is the empty-id filler: {partial_row:?}"
    );
    assert!(
        partial_tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
        "and the sha follows it: {partial_row:?}"
    );
}

/// The anchor walk stops early — once every displayed commit is answered, or
/// once the chain predates the oldest of them — so what it reports must not
/// depend on how far it happened to walk. Same chain, two window sizes, same
/// letters.
#[test]
fn log_segment_tips_ignore_how_far_the_walk_went() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Segment User");
    fx.set_config("user.email", "segment@test");
    fx.write("a.txt", "a\n");
    fx.commit("root");

    // Three ff-made commits, each with snapshots piled on top, so every
    // anchor sits well below newer chain links.
    let mut landed = Vec::new();
    for round in 0..3 {
        for noise in 0..3 {
            fx.write("noise.txt", &format!("{round}-{noise}\n"));
            assert!(ff(&fx, &[]).status.success());
        }
        fx.write("a.txt", &format!("round {round}\n"));
        assert!(
            ff(&fx, &["commit", "-m", &format!("landed {round}")])
                .status
                .success()
        );
        landed.push(fx.git(&["rev-parse", "HEAD"]).trim().to_string());
    }

    let letters_for = |text: &str, sha: &str| -> String {
        let row = text
            .lines()
            .find(|line| line.starts_with('●') && line.contains(&sha[..7]))
            .unwrap_or_else(|| panic!("no ● row for {sha}: {text:?}"));
        row.split_whitespace().nth(1).unwrap().to_string()
    };

    // The tree is clean and stays clean, so these reads add no snapshots and
    // the two windows see the identical chain.
    let full = stdout(&ff(&fx, &["log"]));
    let narrow = stdout(&ff(&fx, &["log", "-n", "1"]));

    for sha in &landed {
        let id = letters_for(&full, sha);
        assert!(
            id.chars().all(|c| ('k'..='z').contains(&c)),
            "ff-made commit keeps its anchor behind newer snapshots: {id:?}"
        );
    }
    assert_eq!(
        letters_for(&narrow, landed.last().unwrap()),
        letters_for(&full, landed.last().unwrap()),
        "-n 1 and the full log agree on the newest commit's anchor"
    );
}

/// Three commits on main, clean tree, identity configured — the shape every
/// `-r` test below reads.
fn three_commits() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Revset User");
    fx.set_config("user.email", "revset@test");
    for n in ["one", "two", "three"] {
        fx.write("a.txt", &format!("{n}\n"));
        fx.commit(n);
    }
    fx
}

fn bullet_rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with('●')).collect()
}

/// The subject of each ● row, which the renderer puts on the continuation
/// line beneath it.
fn bullet_subjects(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('●'))
        .map(|(i, _)| lines[i + 1].trim_start_matches('│').trim().to_string())
        .collect()
}

/// `-r` replaces where the rows come from and nothing else: the same renderer,
/// the same columns, a different set.
#[test]
fn log_revisions_narrow_the_rows() {
    let fx = three_commits();

    let all = stdout(&ff(&fx, &["log"]));
    assert_eq!(bullet_rows(&all).len(), 3, "{all:?}");

    let narrowed = stdout(&ff(&fx, &["log", "-r", "main"]));
    assert_eq!(
        bullet_subjects(&narrowed),
        vec!["three".to_string()],
        "one revision, one row — main's tip: {narrowed:?}"
    );

    // The long spelling is the same flag.
    assert_eq!(
        stdout(&ff(&fx, &["log", "--revisions", "main"])),
        narrowed,
        "-r and --revisions are one flag"
    );
}

/// The whole point of the membership rule: `ff log -r main` is a question
/// about main, and main does not contain the open change.
#[test]
fn log_revisions_without_the_open_change_print_no_at_row() {
    let fx = three_commits();

    let narrowed = stdout(&ff(&fx, &["log", "-r", "main"]));
    assert!(
        !narrowed.lines().any(|l| l.starts_with('@')),
        "no @ row when the set excludes it: {narrowed:?}"
    );

    // And it comes back when the set does contain it.
    let with_open = stdout(&ff(&fx, &["log", "-r", "::@"]));
    assert!(
        with_open.lines().next().unwrap().starts_with('@'),
        "@ heads the rows when it is a member: {with_open:?}"
    );
}

/// `--commits` is the plain history view of whatever set it is given, so the
/// two flags compose rather than conflict.
#[test]
fn log_revisions_compose_with_the_commits_view() {
    let fx = three_commits();

    let text = stdout(&ff(&fx, &["log", "--commits", "-r", "main"]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one revision, one row: {text:?}");
    assert!(lines[0].contains("three"), "{text:?}");

    let out = ff(&fx, &["log", "--commits", "-r", "main", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["commits"].as_array().unwrap().len(), 1);
    assert!(d.get("open").is_none(), "no open key in commits view");
}

/// `-n` bounds the commit rows, exactly as it does without `-r`. The `@` row
/// is not one of them, so a set containing the open change still shows it.
#[test]
fn log_revisions_respect_the_row_limit() {
    let fx = three_commits();

    let text = stdout(&ff(&fx, &["log", "-r", "::@", "-n", "2"]));
    assert!(text.lines().next().unwrap().starts_with('@'), "{text:?}");
    assert_eq!(
        bullet_subjects(&text),
        vec!["three".to_string(), "two".to_string()],
        "-n 2 bounds the commit rows and the @ row is not one of them: {text:?}"
    );
}

/// `--ops` is a removal, not a rename: `ff op log` is a different command
/// with a different output shape, so typing the old flag is answered with a
/// redirect rather than a bare "unexpected argument".
#[test]
fn log_ops_redirects_to_the_op_family() {
    let fx = three_commits();
    let out = ff(&fx, &["log", "--ops", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["error"]["id"], "usage/bad-flags");
    let hints = v["error"]["exits"].as_array().expect("exits");
    assert!(
        hints.iter().any(|h| h == "ff op log"),
        "the redirect names the verb that runs: {v}"
    );

    // And the global --session is a tag, never a filter: it rides ff log
    // without being mistaken for one.
    let out = ff(&fx, &["log", "-r", "main", "--session", "work", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A bad revset surfaces as its own coded refusal, not as an empty log.
#[test]
fn log_revisions_surface_revset_errors() {
    let fx = three_commits();
    for (src, id) in [
        ("nosuchbranch", "usage/revset-unknown-revision"),
        ("@^", "usage/revset-open-suffix"),
        ("main...trunk", "usage/revset-no-symmetric-difference"),
    ] {
        let out = ff(&fx, &["log", "-r", src, "--json"]);
        assert!(!out.status.success(), "{src} must fail");
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(v["error"]["id"], id, "{src}");
    }

    // And without --json it is prose on stderr, stdout untouched.
    let out = ff(&fx, &["log", "-r", "nosuchbranch"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("ff: "), "{stderr:?}");
}

/// The JSON contract under `-r`: `commits` unchanged, `open` still always
/// present, and null exactly when the set excludes the open change.
#[test]
fn log_revisions_json_shape() {
    let fx = three_commits();

    let out = ff(&fx, &["log", "-r", "main", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    let commits = d["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["subject"], "three");
    assert!(commits[0]["short_id"].is_string(), "row shape unchanged");
    assert!(commits[0].get("session").is_some(), "per-row session kept");
    assert!(
        d.get("open").is_some() && d["open"].is_null(),
        "open present and null when @ is not a member: {d}"
    );

    // A set that does contain it gets the same object as ever.
    let out = ff(&fx, &["log", "-r", "::@", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["commits"].as_array().unwrap().len(), 3);
    assert_eq!(d["open"]["branch"], "main");
    assert!(d["open"]["clean"].is_boolean());
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
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "two\n");
    assert!(ff(&fx, &[]).status.success());

    let out = ff(&fx, &["evolog"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    // The tree did not change since the second capture, so this run is a
    // NoOp and the row count stays at the two captures above.
    assert_eq!(lines.len(), 2, "snapshot rows only: {text:?}");

    let out = ff(&fx, &["evolog", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let snaps = v["data"]["snapshots"].as_array().unwrap();
    assert_eq!(snaps.len(), 2);
    // Newest first, and the human rows lead with the letters spellings of
    // those same ids in the same order — the message is gone, so the ids
    // are what ties the two surfaces together.
    assert!(
        snaps[0]["time"].as_i64().unwrap() >= snaps[1]["time"].as_i64().unwrap(),
        "snapshots are newest first: {v:?}"
    );
    assert_eq!(
        lines[0].split_whitespace().next().unwrap(),
        letters8(snaps[0]["id"].as_str().unwrap()),
        "row 0 carries the newest id: {text:?}"
    );
    assert_eq!(
        lines[1].split_whitespace().next().unwrap(),
        letters8(snaps[1]["id"].as_str().unwrap()),
        "row 1 carries the older id: {text:?}"
    );

    for line in &lines {
        let token = line.split_whitespace().next().unwrap();
        assert_eq!(token.len(), 8, "letters8 id column: {line:?}");
        assert!(
            token.chars().all(|c| ('k'..='z').contains(&c)),
            "letters alphabet: {token:?}"
        );
    }

    assert!(
        snaps[0]["id"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "JSON ids stay hex"
    );
    // The chain walk leaves short_id empty and the display paths fill it;
    // this is the guard against an unfilled row reaching a reader.
    for snap in snaps {
        let (id, short) = (
            snap["id"].as_str().unwrap(),
            snap["short_id"].as_str().unwrap(),
        );
        assert!(!short.is_empty(), "short_id is filled: {snap}");
        assert!(id.starts_with(short), "short_id abbreviates id: {snap}");
    }
}

/// A letters id copied from evolog output round-trips into `ff restore --at`.
#[test]
fn restore_accepts_letters_id_from_evolog() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["evolog"]);
    let text = stdout(&out);
    // Capture-first: evolog captured the tree as it stood — "diverged" —
    // before it printed, so the row we want (the one holding "captured\n")
    // is the *older* capture, row 1, not the newest row 0.
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "exactly two captures: {text:?}");
    let letters = lines[1]
        .split_whitespace()
        .next()
        .expect("letters id leads the row")
        .to_string();

    let out = ff(&fx, &["restore", "--all", "--at-op", &letters]);
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

    // The capture holding "captured" is an operation, so it is named in the
    // operation address space, and `@` names it: restore resolves its source
    // BEFORE taking its own pre-restore capture, so `@` still means the
    // timeline as the user just saw it. Bare `restore --all` now means the
    // commit under the open change — a different answer, checked below.
    let out = ff(&fx, &["restore", "--all", "--at-op", "@"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.starts_with("restored from "), "header: {text:?}");
    assert!(text.contains("restored  a.txt"), "file list: {text:?}");
    assert!(
        text.trim_end().ends_with("undo: ff undo"),
        "undo hint: {text:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );

    // Bare restore --all takes the whole tree back to the commit under the
    // open change: "captured" was never committed, so it goes.
    let out = ff(&fx, &["restore", "--all"]);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n"
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
    let d = &v["data"];
    // The origin says which address space named the source, so a reader
    // never has to infer it from the shape of an id.
    assert_eq!(d["origin"]["space"], "commit");
    assert!(d["origin"]["id"].is_string());
    assert_eq!(d["restored"][0], "a.txt");
    assert_eq!(d["undo"], "ff undo");
    assert!(d["pre_op"].is_string(), "pre-restore capture recorded");
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
    // One log, so one line: retention acts on the log, and a branch pointer
    // is a place in it rather than a chain of its own.
    assert!(
        text.contains("nothing to drop") && text.contains("operations kept"),
        "the log reports its own kept count: {text:?}"
    );
    assert!(
        !text.contains("main:"),
        "no per-branch retention row: {text:?}"
    );

    let out = ff(&fx, &["trim", "--dry-run", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["dry_run"], true);
    assert_eq!(d["pointers"][0]["branch"], "main");
    assert_eq!(d["pointers"][0]["dropped"], 0);
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

/// Running `ff log --json` twice on the same dirty tree must produce the same
/// pending hash (no-op pre-captures must not move it). A further tree change
/// must produce a different hash. Dropping identity makes pending null.
#[test]
fn log_pending_hash_stability() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    // Two runs on the same dirty tree → same pending hash.
    let out1 = ff(&fx, &["log", "--json"]);
    let v1: serde_json::Value = serde_json::from_str(&stdout(&out1)).unwrap();
    let open1 = &v1["data"]["open"];

    let out2 = ff(&fx, &["log", "--json"]);
    let v2: serde_json::Value = serde_json::from_str(&stdout(&out2)).unwrap();
    let open2 = &v2["data"]["open"];

    let pending1 = open1["pending"].as_str().expect("pending is a string");
    assert_eq!(pending1.len(), 40, "40-char hex");
    assert!(pending1.chars().all(|c| c.is_ascii_hexdigit()), "ascii hex");

    let pending_short1 = open1["pending_short"]
        .as_str()
        .expect("pending_short is a string");
    assert_eq!(pending_short1, &pending1[..8]);

    assert_eq!(
        open2["pending"].as_str().unwrap(),
        pending1,
        "pending hash stable across runs"
    );

    // Edit again → hash changes.
    fx.write("a.txt", "more\n");
    let out3 = ff(&fx, &["log", "--json"]);
    let v3: serde_json::Value = serde_json::from_str(&stdout(&out3)).unwrap();
    assert_ne!(
        v3["data"]["open"]["pending"].as_str().unwrap(),
        pending1,
        "pending hash differs after tree change"
    );

    // Drop identity → pending and pending_short become null.
    fx.git(&["config", "--unset", "user.name"]);
    fx.git(&["config", "--unset", "user.email"]);
    let out4 = ff(&fx, &["log", "--json"]);
    assert!(out4.status.success());
    let v4: serde_json::Value = serde_json::from_str(&stdout(&out4)).unwrap();
    assert!(
        v4["data"]["open"]["pending"].is_null(),
        "pending null without identity"
    );
    assert!(
        v4["data"]["open"]["pending_short"].is_null(),
        "pending_short null without identity"
    );
}

/// `ff commit` on a clean tree with a pending description refuses: the
/// description does not make a change, nothing is written, and the
/// description is still there afterwards.
#[test]
fn commit_refuses_clean_tree_keeps_the_pending_description() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Describe while clean.
    let out = ff(&fx, &["describe", "-m", "planned work"]);
    assert!(out.status.success());

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit"]);
    assert_eq!(out.status.code(), Some(1), "a clean tree refuses");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "stderr names the refusal: {err}"
    );
    assert!(
        err.contains("the pending description stays put"),
        "stderr reassures the description survives: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");

    // The description is still pending.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["data"]["open"]["subject"].as_str(),
        Some("planned work"),
        "subject still pending"
    );
}

/// `ff commit -m` on a clean tree with no pending description refuses: the
/// flag message is discarded with the refusal, and nothing is written.
#[test]
fn commit_refuses_clean_tree_with_message_flag() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit", "-m", "checkpoint"]);
    assert_eq!(out.status.code(), Some(1), "a clean tree refuses");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "stderr names the refusal: {err}"
    );
    assert!(
        !err.contains("the pending description stays put"),
        "no pending description, so no reassurance clause: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");
}

/// `ff commit` on a clean tree with no description refuses: exit 1,
/// stderr naming the refusal, rev-list count unchanged.
#[test]
fn commit_totally_empty_refuses() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit"]);
    assert_eq!(out.status.code(), Some(1), "exit 1 — the refusal");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "refusal message: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");
}

/// `ff start` never creates a commit: a described change stays pending,
/// the description does not become a commit, and the new branch opens clean.
#[test]
fn start_never_commits() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Describe something.
    let out = ff(&fx, &["describe", "-m", "planned work"]);
    assert!(out.status.success());

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    // start does not commit.
    let out = ff(&fx, &["start"]);
    assert!(out.status.success());

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");

    // The description did not become a commit — HEAD is still "init".
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s"]).trim(),
        "init",
        "no commit was created for the description"
    );
}

/// Two consecutive bare `ff start` runs on a clean tree produce two distinct
/// new branches; neither is a no-op.
#[test]
fn start_always_mints() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out1 = ff(&fx, &["start", "--json"]);
    assert!(out1.status.success());
    let v1: serde_json::Value = serde_json::from_str(&stdout(&out1)).unwrap();
    let branch1 = v1["data"]["start"]["minted"].as_str().unwrap();

    let out2 = ff(&fx, &["start", "--json"]);
    assert!(out2.status.success());
    let v2: serde_json::Value = serde_json::from_str(&stdout(&out2)).unwrap();
    let branch2 = v2["data"]["start"]["minted"].as_str().unwrap();

    assert_ne!(branch1, branch2, "two starts produce two distinct branches");
}

/// `ff update` on an unofficial (test) build refuses before any network:
/// classification precedes the API call, so this is hermetic.
#[test]
fn update_on_unofficial_build_advises_cargo() {
    let fx = Fixture::new();
    let out = ff(&fx, &["update"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr.clone()).expect("utf-8 stderr");
    assert!(err.contains("ff: ff was built from source"), "got: {err}");
    assert!(
        err.contains("cargo install --git https://github.com/tyler-johnson/fufu ff-cli"),
        "got: {err}"
    );
}

/// The bold prefix is the only consumer of unique-prefix lengths, so a run
/// that cannot emit ANSI must not build the id index to compute them. This
/// pins the invariant from the observable side: an uncolored view leaves no
/// index behind, a colored one builds it. If someone ever makes those lengths
/// matter without color, this fails rather than silently rendering `1`.
#[test]
fn uncolored_views_do_not_build_the_id_index() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // A log deep enough that the append path needs an existing file, and then
    // a clean tree so neither run below captures anything.
    for i in 0..4 {
        fx.write("a.txt", &format!("v{i}\n"));
        assert!(ff(&fx, &[]).status.success());
    }
    fx.commit("settle");
    assert!(ff(&fx, &[]).status.success());

    // One log means one index file, not one per branch.
    let index = fx.path().join(".git/fufu/ops/live");
    std::fs::remove_file(&index).expect("remove index");

    // stdout here is a pipe, so anstream resolves color to never.
    let out = ff(&fx, &["log", "-n", "5"]);
    assert!(out.status.success(), "uncolored log should succeed");
    assert!(
        !index.exists(),
        "an uncolored view must not build the id index"
    );

    let out = ff_colored(&fx, &["log", "-n", "5"]);
    assert!(out.status.success(), "colored log should succeed");
    assert!(
        index.exists(),
        "a colored view builds the index it needs to embolden with"
    );
    assert!(
        stdout(&out).contains('\u{1b}'),
        "forced color really did emit ANSI, so the assertion above means something"
    );
}

#[test]
fn version_names_the_build() {
    let out = ff_at(&std::env::temp_dir(), &["--version"]);

    assert!(
        out.status.success(),
        "exit status: {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let out_str = stdout(&out).trim_end().to_string();
    let mut lines = out_str.lines();

    // Line one names the tool by its full name, not by the two letters it is
    // typed as: `ff` is not a searchable string, and this is the output a bug
    // report gets pasted from.
    let first = lines.next().unwrap_or_default();
    let prefix = format!("fufu {}", env!("CARGO_PKG_VERSION"));
    assert!(
        first.starts_with(&prefix),
        "stdout did not start with \"{prefix}\": {out_str}"
    );

    // Line two is where to go next, and it comes from the manifest rather
    // than from a literal in the source.
    assert_eq!(
        lines.next(),
        Some(env!("CARGO_PKG_REPOSITORY")),
        "second line is the project's home: {out_str}"
    );
    assert_eq!(lines.next(), None, "two lines and no more: {out_str}");

    let rest = &first[prefix.len()..];

    if !rest.is_empty() {
        assert!(
            rest.starts_with(" (") && rest.ends_with(')'),
            "build info should be parenthesised: {rest:?}"
        );

        let inner = &rest[2..rest.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ' ').collect();
        assert_eq!(
            parts.len(),
            2,
            "build info inner should have exactly two space-separated parts: {inner:?}"
        );

        let sha = parts[0];
        assert!(
            sha.len() >= 7
                && sha
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "sha part is not 7+ lowercase hex: {sha:?}"
        );

        let date = parts[1];
        assert!(
            date.len() == 10
                && date.as_bytes()[4] == b'-'
                && date.as_bytes()[7] == b'-'
                && date
                    .chars()
                    .enumerate()
                    .all(|(i, c)| (i == 4 || i == 7) || c.is_ascii_digit()),
            "date part is not YYYY-MM-DD: {date:?}"
        );
    }

    assert!(
        out.stderr.is_empty(),
        "stderr was not empty: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The three spellings of one question. `-v` is the verb itself, so it
/// cannot drift from `ff version`, and `-V` — what almost every other tool
/// spells this — must be answered rather than met with clap's
/// unknown-argument error.
#[test]
fn the_version_is_asked_three_ways_and_answered_once() {
    let tmp = std::env::temp_dir();

    let long = ff_at(&tmp, &["--version"]);
    let short = ff_at(&tmp, &["-v"]);
    let verb = ff_at(&tmp, &["version"]);
    for out in [&long, &short, &verb] {
        assert!(out.status.success(), "exit 0: {:?}", out.status);
    }

    assert_eq!(stdout(&short), stdout(&long), "-v is --version");
    // The flag is the verb, so the spellings match line for line.
    let line = stdout(&long);
    assert!(
        stdout(&verb).starts_with(line.trim_end()),
        "ff version does not lead with the flag's line: {:?} vs {line:?}",
        stdout(&verb)
    );

    // `-V` is gone as a spelling and present as an answer.
    let shouted = ff_at(&tmp, &["-V"]);
    assert!(!shouted.status.success(), "-V no longer prints a version");
    let err = String::from_utf8_lossy(&shouted.stderr).to_string();
    assert!(err.contains("ff -v"), "names the spelling: {err}");
    assert!(err.contains("ff version"), "names the verb: {err}");
}

/// The envelope names the verb that ran. `ff -v --json` settles as the
/// version verb and not as the map, so the flag cannot answer a different
/// question from the verb it spells.
#[test]
fn the_version_flag_takes_the_envelope() {
    let out = ff_at(&std::env::temp_dir(), &["-v", "--json"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(
        v["cmd"], "version",
        "the flag settled as the verb, not the map"
    );
    assert_eq!(v["data"]["version"], env!("CARGO_PKG_VERSION"));
}

/// The flag does not ride another verb: `-v status` is two commands on one
/// line, refused with the two spellings that would each be right alone.
#[test]
fn the_version_flag_does_not_ride_another_verb() {
    let out = ff_at(&std::env::temp_dir(), &["-v", "status"]);
    assert_eq!(out.status.code(), Some(2), "usage error: {:?}", out.status);
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("ff -v"), "names the flag: {err}");
    assert!(err.contains("ff version"), "names the verb: {err}");
}

/// The envelope carries the line as fields, so a caller never takes the
/// display string apart.
#[test]
fn version_json_splits_the_line_into_fields() {
    let out = ff_at(&std::env::temp_dir(), &["version", "--json"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "version");
    assert_eq!(v["data"]["version"], env!("CARGO_PKG_VERSION"));

    // Commit and date are both recorded or both null — never one alone, which
    // is what the build script's "both or neither" rule buys.
    let commit = &v["data"]["commit"];
    let date = &v["data"]["date"];
    assert_eq!(
        commit.is_null(),
        date.is_null(),
        "half a provenance: {commit} / {date}"
    );
    if let Some(commit) = commit.as_str() {
        assert!(
            commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()),
            "not a short sha: {commit}"
        );
        assert_eq!(date.as_str().map(str::len), Some(10), "not YYYY-MM-DD");
        // And the display line is built from exactly these two.
        let line = stdout(&ff_at(&std::env::temp_dir(), &["-v"]));
        assert!(line.contains(commit), "the line drops the commit: {line}");
    }

    // The update lane always reports one of its four states, and names a tag
    // only when there is one to name.
    let status = v["data"]["update"]["status"].as_str().expect("a status");
    assert!(
        ["unofficial", "unchecked", "available", "current"].contains(&status),
        "unknown update status: {status}"
    );
    assert_eq!(
        v["data"]["update"]["latest"].is_null(),
        status != "available",
        "a tag is named exactly when one is available"
    );
}

/// `ff config` lists the theme setting and its three values.
#[test]
fn config_lists_theme() {
    let fx = Fixture::new();
    let out = ff(&fx, &["config"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("theme"), "missing theme: {text}");
    assert!(text.contains("muted"), "missing muted: {text}");
    assert!(text.contains("vivid"), "missing vivid: {text}");
    assert!(text.contains("terminal"), "missing terminal: {text}");
}

/// `ff config theme <v>` accepts each valid value and echoes it back.
#[test]
fn config_theme_accepts_each_value() {
    let fx = Fixture::new();
    for value in &["muted", "vivid", "terminal"] {
        let out = ff(&fx, &["config", "theme", value]);
        assert!(
            out.status.success(),
            "setting theme to {value} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = ff(&fx, &["config", "theme"]);
        assert!(out.status.success());
        assert_eq!(
            stdout(&out).trim(),
            *value,
            "reading back theme after setting {value}"
        );
    }
}

/// `ff config theme VIVID` normalizes to lowercase on read.
#[test]
fn config_theme_normalizes_case() {
    let fx = Fixture::new();
    let out = ff(&fx, &["config", "theme", "VIVID"]);
    assert!(out.status.success(), "uppercase set failed");
    let out = ff(&fx, &["config", "theme"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out).trim(), "vivid", "normalized to lowercase");
}

/// `ff config theme neon` rejects an unknown value with exit code 2.
#[test]
fn config_theme_rejects_unknown() {
    let fx = Fixture::new();
    let out = ff(&fx, &["config", "theme", "neon"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown theme value should exit 2"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("muted") && err.contains("vivid") && err.contains("terminal"),
        "stderr should name valid values: {err}"
    );
}

/// Unset theme falls back to the default `muted`.
#[test]
fn config_theme_default_is_muted() {
    let fx = Fixture::new();
    let out = ff(&fx, &["config", "theme"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        "muted",
        "unset theme reports muted default"
    );
}

/// Every ``--json`` output carries the versioned envelope.
#[test]
fn json_output_carries_the_envelope() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1, "contract version");
    assert_eq!(v["cmd"], "status", "command name");
    assert!(v["data"].is_object(), "data is an object");
    assert!(
        v["data"].get("changes").is_some(),
        "data contains the changes key"
    );
}

/// The ``cmd`` field matches the verb for every JSON-emitting command.
#[test]
fn json_envelope_names_each_command() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    for (args, expected_cmd) in [
        (["status", "--json"], "status"),
        (["log", "--json"], "log"),
        (["evolog", "--json"], "evolog"),
        (["doctor", "--json"], "doctor"),
    ] {
        let out = ff(&fx, &args);
        let text = stdout(&out);
        // doctor may exit 1 with findings; it still emits an envelope.
        let v: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("valid json from {args:?}: {err}: {text}"));
        assert_eq!(v["cmd"], expected_cmd, "cmd mismatch for {:?}", args);
    }
}

/// ``--json`` output is exactly one line terminated by a single newline.
#[test]
fn json_output_is_one_line() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline: {:?}",
        text
    );
}

/// A sub-mode of one verb shares its name; a different verb does not. So
/// ``ff log --commits`` is still ``log``, while the operation log — a
/// different command with a different output shape — stamps ``op log``.
/// ``ff session`` is the anti-precedent: two shapes under one name.
#[test]
fn each_shape_carries_its_own_envelope_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "so there is an operation to show\n");
    assert!(ff(&fx, &[]).status.success());

    for (args, name) in [
        (vec!["log", "--commits", "--json"], "log"),
        (vec!["op", "log", "--json"], "op log"),
        (vec!["op", "show", "--json"], "op show"),
    ] {
        let out = ff(&fx, &args);
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
        assert_eq!(v["cmd"], name, "cmd for {args:?}");
    }
}

#[test]
fn error_json_uses_the_envelope() {
    // Running ff status --json outside any repository provokes a discovery error.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["ff"], 1, "envelope version");
    assert!(v.get("error").is_some(), "has error object");
    assert!(!v["error"]["id"].is_null(), "error.id is non-empty");
    assert!(
        !v["error"]["message"].is_null(),
        "error.message is non-empty"
    );
    assert!(v.get("data").is_none(), "has no data key");
}

/// The exits a raise site never wrote down. Most `Error::coded` calls pass
/// none — the way out belongs to the id, and the registry has held it all
/// along — so the failure reads what `ff explain` reads. Before this, `no
/// branch named x` was a dead end, and the next thing tried was git.
#[test]
fn a_failure_with_no_exits_of_its_own_borrows_the_registrys() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out = ff(&fx, &["switch", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no branch named nope"), "{stderr}");
    assert!(stderr.contains("try:"), "{stderr}");
    assert!(stderr.contains("ff branch list"), "{stderr}");
}

/// Both surfaces read the one registry, so a machine is told what a terminal
/// would be.
#[test]
fn the_borrowed_exits_reach_the_json_envelope_too() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out = ff(&fx, &["switch", "nope", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "branch/not-found");
    let exits = v["error"]["exits"].as_array().expect("exits array");
    assert!(
        exits.iter().any(|e| e == "ff branch list"),
        "envelope carries the registry's exits: {exits:?}"
    );
}

/// The floor under both: an id whose registry entry has no exits either is
/// still not a dead end, because the entry itself is the thing to read.
#[test]
fn an_id_with_no_exits_anywhere_names_its_own_explanation() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let bare = dir.path().join("bare.git");
    std::process::Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&bare)
        .output()
        .expect("git init --bare");
    let out = ff_at(&bare, &["status"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ff explain repo/bare"),
        "a coded failure always leaves somewhere to go: {stderr}"
    );
}

#[test]
fn human_error_lists_its_exits() {
    let fx = Fixture::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["describe"])
        .env("FF_NONINTERACTIVE", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("try:"), "stderr contains try:");
    assert!(
        stderr.contains("ff describe -m <msg>"),
        "stderr contains hint"
    );
}

#[test]
fn uncoded_errors_report_as_internal_and_exit_one() {
    // Detached HEAD causes ff describe to return Error::coded("repo/detached", ...).
    // -m bypasses the non-interactive guard so the detached-HEAD path is reached.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["checkout", "--detach", "HEAD"]);
    let out = ff(&fx, &["describe", "-m", "test", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["error"]["id"], "repo/detached");
}

/// `ff explain <id>` works outside a repository and renders the entry.
#[test]
fn explain_known_id_works_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "repo/bare"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("repo/bare"));
    assert!(text.contains("bare repository"));
}

/// `ff explain --list` renders all entries, works outside a repository.
#[test]
fn explain_list_works_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "--list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("repo/bare"));
    assert!(text.contains("branch/not-found"));
    assert!(text.contains("internal"));
}

/// `ff explain <id> --json` emits the versioned envelope with entry data.
#[test]
fn explain_json_single_entry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "branch/exists", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1, "envelope version");
    assert_eq!(v["cmd"], "explain");
    assert_eq!(v["data"]["id"], "branch/exists");
    assert!(v["data"]["summary"].is_string());
    assert!(v["data"]["detail"].is_string());
    assert!(v["data"]["exits"].is_array());
}

/// `ff explain --list --json` emits an array of entries.
#[test]
fn explain_json_list() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "--list", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "explain");
    let entries = v["data"]["entries"].as_array().expect("entries is array");
    assert!(entries.len() >= 16, "registry has entries");
    for entry in entries {
        assert!(entry["id"].is_string(), "each entry has id");
        assert!(entry["summary"].is_string(), "each entry has summary");
    }
}

/// `ff explain <unknown-id>` exits 2 (usage/) with usage/unknown-error-id.
#[test]
fn explain_unknown_id_exits_two() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "nonexistent/foo", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/unknown-error-id");
}

/// `ff explain` with no arguments exits 2 (usage error).
#[test]
fn explain_no_args_exits_two() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain"]);
    assert_eq!(out.status.code(), Some(2));
}

/// Human explain output includes try: hints when the entry has exits.
#[test]
fn explain_human_includes_hints() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "branch/not-found"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("try:"));
    assert!(text.contains("ff branch"));
}

/// Human explain output omits try: block when the entry has no exits.
#[test]
fn explain_human_no_hints_when_empty() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["explain", "repo/bare"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("try:"), "no exits means no try: block");
}

/// `--json` after the verb parses and emits an envelope. The "before the verb"
/// position (`ff --json status`) does not work because clap's
/// `args_conflicts_with_subcommands` does not exempt `global = true` args.
#[test]
fn json_flag_parses_after_the_verb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success(), "exit 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "status");

    let out = ff(&fx, &["log", "--json"]);
    assert!(out.status.success(), "exit 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "log");
}

/// `--json` is accepted by every verb (clap does not reject it with exit 2).
#[test]
fn json_is_accepted_by_every_verb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    for verb in ["status", "log", "evolog", "doctor", "config", "branch"] {
        let out = ff(&fx, &[verb, "--json"]);
        // A clap usage error exits 2 with "unexpected argument". We assert
        // that does NOT happen: the flag is accepted.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("unexpected argument"),
            "--json rejected by {verb}: {stderr}"
        );
    }
}

/// `ff -m x status` remains a usage error. clap no longer refuses it — the
/// setting that did also refused the global flags — so main names the one
/// real conflict itself, and it must keep landing as `usage/bad-flags`.
#[test]
fn bare_flags_still_conflict_with_subcommands() {
    let fx = Fixture::new();
    let out = ff(&fx, &["-m", "x", "status"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "bare ff args must still conflict with subcommands"
    );

    let out = ff(&fx, &["-m", "x", "status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");
}

/// This error is raised before there is a `Ctx` to consult, which is exactly
/// where a rendering can start depending on how early the failure happened
/// rather than on what the caller asked for. Without `--json` it must read as
/// prose on stderr and leave stdout empty, the same as every other failure.
#[test]
fn a_usage_error_without_json_stays_prose_on_stderr() {
    let fx = Fixture::new();
    let out = ff(&fx, &["-m", "x", "status"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "", "stdout must stay empty without --json");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("ff: "),
        "expected prose on stderr, got {stderr:?}"
    );
    assert!(
        !stderr.contains("\"ff\":1"),
        "the machine envelope must not appear without --json: {stderr:?}"
    );
}

/// The globals are `global = true` precisely so they can ride any verb, and
/// `ff --json status` — the spelling DESIGN uses for `--at-op` too — was a
/// clap usage error until the conflict setting came off.
#[test]
fn globals_ride_ahead_of_the_subcommand() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["--json", "status"]);
    assert!(
        out.status.success(),
        "ff --json status must parse: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "status", "envelope names the verb");
    assert!(v["data"].is_object(), "envelope carries a payload");

    // The trailing spelling, which always worked, still does.
    let trailing = ff(&fx, &["status", "--json"]);
    assert!(trailing.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&trailing)).expect("valid json");
    assert_eq!(v["cmd"], "status");

    // And --session rides the same way — it is a tag on the capture this
    // invocation takes, not a verb of its own and not a filter.
    let out = ff(&fx, &["--session", "leading", "op", "log", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "op log");
}
