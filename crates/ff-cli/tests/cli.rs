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
    // `-m` is bare ff's snapshot message: riding another verb is nonsense,
    // and `usage/bad-flags` puts it in the same exit-2 class as clap's own.
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
    assert_eq!(v["data"]["outcome"], "created");
    assert_eq!(v["data"]["branch"], "main");
    assert!(
        v["data"]["id"]
            .as_str()
            .unwrap()
            .starts_with(v["data"]["short_id"].as_str().unwrap())
    );

    let again = ff(&fx, &["--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&again)).unwrap();
    assert_eq!(v["data"]["outcome"], "noop");
    assert_eq!(v["data"]["branch"], "main");
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
        tokens[2].len() == 7 && tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
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
        tokens[2].len() == 7 && tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
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
    fn letters8(hex: &str) -> String {
        const ALPHABET: &[u8; 16] = b"zyxwvutsrqponmlk";
        hex[..8]
            .chars()
            .map(|c| ALPHABET[c.to_digit(16).unwrap() as usize] as char)
            .collect()
    }

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

    // bare's row: blank letters → sha is first token after ●.
    let bare_row = row_of(&bare);
    let bare_tokens: Vec<&str> = bare_row.split_whitespace().collect();
    assert!(
        bare_tokens[1].chars().all(|c| c.is_ascii_hexdigit()),
        "bare row has blank letters, sha visible: {bare_row:?}"
    );

    // partial's row: blank letters → sha is first token after ●.
    let partial_row = row_of(&partial);
    let partial_tokens: Vec<&str> = partial_row.split_whitespace().collect();
    assert!(
        partial_tokens[1].chars().all(|c| c.is_ascii_hexdigit()),
        "partial row has blank letters, sha visible: {partial_row:?}"
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
    let snaps = v["data"]["snapshots"].as_array().unwrap();
    assert_eq!(snaps.len(), 2);
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
    let d = &v["data"];
    assert!(d["target"]["id"].is_string());
    assert_eq!(d["restored"][0], "a.txt");
    assert_eq!(d["undo"], "ff restore --all");
    assert!(
        d["pre_snapshot"].is_string(),
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
    let d = &v["data"];
    assert_eq!(d["dry_run"], true);
    assert_eq!(d["chains"][0]["branch"], "main");
    assert_eq!(d["chains"][0]["dropped"], 0);
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
    assert_eq!(pending_short1, &pending1[..7]);

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

/// `ff describe -m` sets a description; `ff commit` with no `-m` on a clean
/// tree mints an empty commit using that description, then the description is
/// consumed.
#[test]
fn commit_closes_described_empty_change() {
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

    // Commit without -m should close using the description.
    let out = ff(&fx, &["commit"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("closed "));

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before + 1);

    assert_eq!(fx.git(&["log", "-1", "--format=%s"]).trim(), "planned work");

    // Empty commit: HEAD tree == HEAD~1 tree.
    assert_eq!(
        fx.git(&["rev-parse", "HEAD^{tree}"]).trim(),
        fx.git(&["rev-parse", "HEAD~1^{tree}"]).trim(),
        "empty commit — trees match"
    );

    // Description consumed.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(
        v["data"]["open"]["subject"].is_null(),
        "subject null after consume"
    );
}

/// `ff commit -m` on a clean tree with no pending description still lands an
/// empty commit using the flag message.
#[test]
fn commit_clean_with_message_flag_lands_empty() {
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
    assert!(out.status.success());

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before + 1);

    assert_eq!(fx.git(&["log", "-1", "--format=%s"]).trim(), "checkpoint");

    assert_eq!(
        fx.git(&["rev-parse", "HEAD^{tree}"]).trim(),
        fx.git(&["rev-parse", "HEAD~1^{tree}"]).trim(),
        "empty commit — trees match"
    );
}

/// `ff commit` on a clean tree with no description is a no-op: exit 0,
/// informative stdout, rev-list count unchanged.
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
    assert!(out.status.success(), "exit 0 — not an error");
    assert!(
        stdout(&out).contains("nothing to close on main"),
        "refusal message: {:?}",
        stdout(&out)
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
    // A chain deep enough that the append path needs an existing file, and
    // then a clean tree so neither run below captures anything.
    for i in 0..4 {
        fx.write("a.txt", &format!("v{i}\n"));
        assert!(ff(&fx, &[]).status.success());
    }
    fx.commit("settle");
    assert!(ff(&fx, &[]).status.success());

    let index = fx.path().join(".git/fufu/ids/live/main");
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
    let prefix = format!("ff {}", env!("CARGO_PKG_VERSION"));
    assert!(
        out_str.starts_with(&prefix),
        "stdout did not start with \"{prefix}\": {out_str}"
    );

    let rest = &out_str[prefix.len()..];

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

/// ``ff log --ops --json`` and ``ff log --commits --json`` both report
/// ``cmd == "log"`` (sub-modes share the parent verb name).
#[test]
fn log_ops_and_commits_share_the_log_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    for args in [["log", "--ops", "--json"], ["log", "--commits", "--json"]] {
        let out = ff(&fx, &args);
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
        assert_eq!(v["cmd"], "log", "cmd is log for {:?}", args);
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

    for verb in [
        "status", "log", "evolog", "doctor", "config", "branch", "session",
    ] {
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

    // And --session rides the same way.
    let out = ff(&fx, &["--session", "leading", "session", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["data"]["name"], "leading");
}
