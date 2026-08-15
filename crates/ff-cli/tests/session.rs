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
