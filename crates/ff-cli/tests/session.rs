//! Session command integration tests: marker lifecycle, trailer attachment,
//! and environment override. Runs the real `ff` binary against hermetic fixtures.

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

/// Like `ff` but with `FF_SESSION` set.
fn ff_with_session(fx: &Fixture, session: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
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
        .env("FF_SESSION", session)
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

// --- normalize rules (unit-style, via the binary) ---

#[test]
fn normalize_rules() {
    // The normalize function is tested via the session module's own unit
    // tests; here we verify that a name flows through the full CLI path.
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Starting a session with a name that needs normalization.
    let out = ff(&fx, &["session", "start", "Refactor Parser!"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("refactor-parser"),
        "normalized name in output: {text}"
    );

    // A name of only invalid chars becomes a generated name (not an error).
    let out = ff(&fx, &["session", "start", "--", "---"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
}

// --- start then snapshot carries the name ---

#[test]
fn start_then_snapshot_carries_the_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Open a session.
    let out = ff(&fx, &["session", "start", "work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Make a change and take a snapshot.
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Read the snapshot commit message and check the trailer.
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: work"),
        "snapshot message carries session trailer: {msg}"
    );
}

// --- env overrides the marker ---

#[test]
fn env_overrides_the_marker() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Open a session named "a" via the marker.
    let out = ff(&fx, &["session", "start", "a"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Make a change and snapshot with FF_SESSION=b.
    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "b", &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // The trailer should say "b", not "a".
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: b"),
        "env overrides marker: {msg}"
    );
    assert!(
        !msg.contains("fufu-session: a"),
        "marker name should not appear: {msg}"
    );
}

// --- end clears the marker ---

#[test]
fn end_clears_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Open a session.
    let out = ff(&fx, &["session", "start", "work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // End it.
    let out = ff(&fx, &["session", "end"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Make a change and snapshot — no session trailer.
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        !msg.contains("fufu-session:"),
        "no session trailer after end: {msg}"
    );
}

// --- start replaces and reports ---

#[test]
fn start_replaces_and_reports() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Open first session.
    let out = ff(&fx, &["session", "start", "first"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Start a second — should replace and mention the old name.
    let out = ff(&fx, &["session", "start", "second"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("replaced"), "replacement reported: {text}");
    assert!(text.contains("first"), "old name mentioned: {text}");
}

// --- session survives no repo gracefully ---

#[test]
fn session_survives_no_repo_gracefully() {
    // Run outside any repository — should fail like other repo verbs,
    // not panic.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let out = ff_at(tmp.path(), &["session"]);
    assert!(!out.status.success(), "should fail outside a repo");
    let err = stderr(&out);
    assert!(!err.contains("panic"), "no panic in error: {err}");
}

// --- spans: contiguous by name, not merged across a gap ---

#[test]
fn spans_are_contiguous_by_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "0\n");
    fx.commit("init");

    // Oldest span of "work": two snapshots.
    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "1\n");
    ff(&fx, &[]);
    fx.write("a.txt", "2\n");
    ff(&fx, &[]);
    ff(&fx, &["session", "end"]);

    // A gap: a snapshot with no session.
    fx.write("a.txt", "3\n");
    ff(&fx, &[]);

    // Newest span of "work": one snapshot. Same name, but not contiguous
    // with the older span, so it must report separately.
    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "4\n");
    ff(&fx, &[]);

    let out = ff(&fx, &["session", "list", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let spans = v["data"]["spans"].as_array().expect("spans array");
    assert_eq!(spans.len(), 2, "two spans, not one merged: {spans:#?}");
    assert_eq!(spans[0]["name"], "work");
    assert_eq!(spans[0]["snapshots"], 1, "newest span: {spans:#?}");
    assert_eq!(spans[1]["name"], "work");
    assert_eq!(spans[1]["snapshots"], 2, "oldest span: {spans:#?}");
    assert_ne!(
        spans[0]["oldest"], spans[1]["newest"],
        "the gap snapshot must not bridge the two spans"
    );
}

// --- list: newest first, correct counts ---

#[test]
fn list_reports_newest_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "0\n");
    fx.commit("init");

    ff(&fx, &["session", "start", "alpha"]);
    fx.write("a.txt", "1\n");
    ff(&fx, &[]);
    ff(&fx, &["session", "end"]);

    ff(&fx, &["session", "start", "beta"]);
    fx.write("a.txt", "2\n");
    ff(&fx, &[]);
    fx.write("a.txt", "3\n");
    ff(&fx, &[]);

    let out = ff(&fx, &["session", "list", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let spans = v["data"]["spans"].as_array().expect("spans array");
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["name"], "beta", "newest session first: {spans:#?}");
    assert_eq!(spans[0]["snapshots"], 2);
    assert_eq!(spans[1]["name"], "alpha");
    assert_eq!(spans[1]["snapshots"], 1);
}

// --- list: empty is a yes, not a no ---

#[test]
fn list_is_empty_without_sessions() {
    let fx = Fixture::new();
    fx.write("a.txt", "0\n");
    fx.commit("init");

    let out = ff(&fx, &["session", "list", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["data"]["spans"].as_array().expect("array").len(), 0);

    let out = ff(&fx, &["session", "list"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("no sessions on this branch"),
        "human message: {}",
        stdout(&out)
    );
}

// --- log --session: named form narrows ---

#[test]
fn log_session_filter_narrows() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "0\n");
    fx.commit("init");

    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "1\n");
    let out = ff(&fx, &["commit", "-m", "commit under work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    ff(&fx, &["session", "end"]);

    fx.write("a.txt", "2\n");
    let out = ff(&fx, &["commit", "-m", "commit without session"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = ff(&fx, &["log", "--session", "work", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let commits = v["data"]["commits"].as_array().expect("commits array");
    assert_eq!(commits.len(), 1, "only the row from work: {commits:#?}");
    assert_eq!(commits[0]["session"], "work");
    assert_eq!(commits[0]["subject"], "commit under work");
}

// --- log --json: session field always present, null where absent ---

#[test]
fn log_rows_carry_the_session_field() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "0\n");
    fx.commit("init");

    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "1\n");
    let out = ff(&fx, &["commit", "-m", "commit under work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    ff(&fx, &["session", "end"]);

    // No --session flag at all — the field must still be there.
    let out = ff(&fx, &["log", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let commits = v["data"]["commits"].as_array().expect("commits array");
    assert_eq!(commits.len(), 2);
    for c in commits {
        assert!(
            c.as_object()
                .expect("row is an object")
                .contains_key("session"),
            "row missing session key: {c}"
        );
    }
    let under_work = commits
        .iter()
        .find(|c| c["subject"] == "commit under work")
        .expect("work commit present");
    assert_eq!(under_work["session"], "work");
    let init = commits
        .iter()
        .find(|c| c["subject"] == "init")
        .expect("init commit present");
    assert_eq!(init["session"], serde_json::Value::Null);
}

// --- log --session rejects --ops and --commits ---

#[test]
fn log_session_rejects_ops_and_commits() {
    let fx = Fixture::new();
    fx.write("a.txt", "0\n");
    fx.commit("init");

    let out = ff(&fx, &["log", "--ops", "--session", "--json"]);
    assert!(!out.status.success(), "--ops --session must fail");
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");

    let out = ff(&fx, &["log", "--commits", "--session", "--json"]);
    assert!(!out.status.success(), "--commits --session must fail");
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");
}

// --- session diff: reports exactly the span's change ---

#[test]
fn session_diff_reports_the_span_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "line1\n");
    fx.write("b.txt", "before\n");
    fx.commit("init");

    // A change before the session opens — must not appear in the diff.
    fx.write("b.txt", "before-changed\n");
    ff(&fx, &[]);

    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "line1\nline2\n");
    ff(&fx, &[]);
    fx.write("a.txt", "line1\nline2\nline3\n");
    ff(&fx, &[]);
    ff(&fx, &["session", "end"]);

    let out = ff(&fx, &["session", "diff", "work", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let changes = v["data"]["changes"].as_array().expect("changes array");
    assert_eq!(
        changes.len(),
        1,
        "only a.txt changed in the span: {changes:#?}"
    );
    assert_eq!(changes[0]["path"], "a.txt");
    assert_eq!(
        changes[0]["insertions"], 2,
        "two lines added across the span"
    );
    assert!(
        !changes.iter().any(|c| c["path"] == "b.txt"),
        "pre-session edit must not appear: {changes:#?}"
    );
}

// --- session diff: no session open, none named ---

#[test]
fn session_diff_without_a_session_errors() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");

    let out = ff(&fx, &["session", "diff", "--json"]);
    assert!(
        !out.status.success(),
        "must fail with no session and no name"
    );
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/needs-session");
}

// --- session diff: several spans, picks and names the newest ---

#[test]
fn session_diff_picks_the_newest_span() {
    let fx = Fixture::new();
    fx.write("a.txt", "0\n");
    fx.commit("init");

    // Older span of "work".
    ff(&fx, &["session", "start", "work"]);
    fx.write("a.txt", "1\n");
    ff(&fx, &[]);
    ff(&fx, &["session", "end"]);

    // A gap.
    fx.write("b.txt", "gap\n");
    ff(&fx, &[]);

    // Newer span of "work" — a different file changes.
    ff(&fx, &["session", "start", "work"]);
    fx.write("c.txt", "new\n");
    ff(&fx, &[]);
    ff(&fx, &["session", "end"]);

    let out = ff(&fx, &["session", "diff", "work", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let changes = v["data"]["changes"].as_array().expect("changes array");
    let paths: Vec<&str> = changes
        .iter()
        .map(|c| c["path"].as_str().expect("path is a string"))
        .collect();
    assert!(paths.contains(&"c.txt"), "newest span's change: {paths:?}");
    assert!(
        !paths.contains(&"a.txt"),
        "older span's change must not appear: {paths:?}"
    );

    let out = ff(&fx, &["session", "diff", "work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("newest span"),
        "output names which span it used: {}",
        stdout(&out)
    );
}

// --- helpers ---

fn read_ref(repo: &Path, r#ref: &str) -> String {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "--verify", r#ref])
        .output()
        .expect("git rev-parse");
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .trim()
        .to_string()
}

fn git_cat_file_commit(repo: &Path, sha: &str) -> String {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(["cat-file", "-p", sha])
        .output()
        .expect("git cat-file");
    String::from_utf8(out.stdout).expect("utf-8")
}
