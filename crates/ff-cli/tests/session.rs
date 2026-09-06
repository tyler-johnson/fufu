//! Session integration tests: name validation, trailer attachment,
//! environment override, and the read-only session command.
//! Runs the real `ff` binary against hermetic fixtures.

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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

/// Like `ff` but with `FF_SESSION` set.
fn ff_with_session(fx: &Fixture, session: &str, args: &[&str]) -> Output {
    ff_env(fx, &[("FF_SESSION", session)], args)
}

/// Like `ff` but with the given session variables set and every other one
/// scrubbed, so a suite run under an agent sees only what the test names.
fn ff_env(fx: &Fixture, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(fx.path())
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
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

// --- names keep their shape ---

#[test]
fn names_keep_their_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Snapshot with an uppercase name containing spaces and punctuation.
    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "Refactor Parser!", &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Read the snapshot commit message and check the trailer.
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: Refactor Parser!"),
        "snapshot message carries exact session trailer: {msg}"
    );
}

// --- unicode and punctuation survive ---

#[test]
fn unicode_and_punctuation_survive() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "hello 🌍/world", &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: hello 🌍/world"),
        "unicode and slashes survive: {msg}"
    );
}

// --- control characters are refused ---

#[test]
fn control_characters_are_refused() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["--session", "a\nb"])
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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff");

    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code 2 for bad session: stderr={}",
        stderr(&out)
    );
}

// --- over length is refused ---

#[test]
fn over_length_is_refused() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // 129 bytes — too long.
    let long = "a".repeat(129);
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["--session", &long])
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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff");

    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code 2 for over-length: stderr={}",
        stderr(&out)
    );

    // 128 bytes — just fine.
    let ok = "a".repeat(128);
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["--session", &ok])
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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff");

    assert!(
        out.status.success(),
        "128 bytes should succeed: stderr={}",
        stderr(&out)
    );
}

// --- bad env is ignored, not fatal ---

#[test]
fn bad_env_is_ignored_not_fatal() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // FF_SESSION contains a control character — should be ignored, not fatal.
    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "a\nb", &[]);
    assert!(
        out.status.success(),
        "command still exits 0: stderr={}",
        stderr(&out)
    );

    // The snapshot should have no session trailer.
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        !msg.contains("fufu-session:"),
        "no session trailer when env is bad: {msg}"
    );
}

// --- flag beats env ---

#[test]
fn flag_beats_env() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["--session", "b"])
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
        .env("FF_SESSION", "a")
        .output()
        .expect("spawn ff");

    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(msg.contains("fufu-session: b"), "flag value stamped: {msg}");
    assert!(
        !msg.contains("fufu-session: a"),
        "env value should not appear: {msg}"
    );
}

// --- commit carries the session ---

#[test]
fn commit_carries_the_session() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "work", &["commit", "-m", "under session"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // The pre-commit snapshot should carry the session trailer.
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: work"),
        "commit pre-snapshot carries session: {msg}"
    );
}

// --- switch carries the session ---

#[test]
fn switch_carries_the_session() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Create another branch to switch to.
    fx.git(&["branch", "other"]);

    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "work", &["switch", "other"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // The pre-switch snapshot should carry the session trailer.
    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: work"),
        "switch pre-snapshot carries session: {msg}"
    );
}

// --- the verb is gone; the tag is not ---

/// `ff session` was a verb for listing what is a tag, and whoever sets a tag
/// already knows its name — so the verb went, subcommands and all. What
/// survives is the tag itself, readable where it is actually stored: on the
/// operation it stamped.
#[test]
fn the_session_verb_is_gone_and_the_tag_rides_the_operation() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    for args in [
        &["session"][..],
        &["session", "list"][..],
        &["session", "diff"][..],
        &["session", "start", "x"][..],
    ] {
        let out = ff(&fx, args);
        assert!(!out.status.success(), "ff {args:?} must not resolve");
        assert!(
            stderr(&out).contains("unrecognized subcommand"),
            "ff {args:?}: {}",
            stderr(&out)
        );
    }

    // The tag itself is on the row it stamped, and `--json` carries it.
    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "my-session", &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let out = ff(&fx, &["op", "log", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let ops = v["data"]["ops"].as_array().expect("ops array");
    assert!(
        ops.iter().any(|op| op["session"] == "my-session"),
        "the tag rides the row: {v}"
    );
}

// --- env overrides no session ---

#[test]
fn env_provides_session_for_snapshot() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = ff_with_session(&fx, "env-session", &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: env-session"),
        "env session stamped on snapshot: {msg}"
    );
}

// --- the client's session is the third source ---

const CLIENT: &str = "95b36d9d-efdc-4564-9b06-91842f51ef6b";

/// With neither `--session` nor `FF_SESSION`, the session the client that
/// launched this `ff` is running is the tag, so a shell verb under an agent
/// carries the session its hook captures do.
#[test]
fn the_clients_session_is_read_when_fufu_names_none() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = ff_env(&fx, &[("CLAUDE_CODE_SESSION_ID", CLIENT)], &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains(&format!("fufu-session: {CLIENT}")),
        "the client's session is stamped on the snapshot: {msg}"
    );
}

/// The client's session is the lowest of the three sources: `FF_SESSION`
/// wins over it, and `--session` wins over both.
#[test]
fn fufus_own_session_wins_over_the_clients() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.write("a.txt", "changed\n");
    let out = ff_env(
        &fx,
        &[
            ("FF_SESSION", "from-env"),
            ("CLAUDE_CODE_SESSION_ID", CLIENT),
        ],
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let repo = fx.path();
    let msg = git_cat_file_commit(&repo, &read_ref(&repo, "refs/fufu/snap/main"));
    assert!(
        msg.contains("fufu-session: from-env"),
        "FF_SESSION beats the client's variable: {msg}"
    );

    fx.write("a.txt", "changed again\n");
    let out = ff_env(
        &fx,
        &[
            ("FF_SESSION", "from-env"),
            ("CLAUDE_CODE_SESSION_ID", CLIENT),
        ],
        &["--session", "from-flag"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let msg = git_cat_file_commit(&repo, &read_ref(&repo, "refs/fufu/snap/main"));
    assert!(
        msg.contains("fufu-session: from-flag"),
        "--session beats both: {msg}"
    );
}

// --- --session flag rides every verb ---

#[test]
fn session_flag_rides_every_verb() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // --session on a commit: the pre-commit snapshot carries it.
    fx.write("a.txt", "changed\n");
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["commit", "--session", "work", "-m", "under flag"])
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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff");
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(
        msg.contains("fufu-session: work"),
        "commit pre-snapshot carries --session flag value: {msg}"
    );
}

// --- --session flag beats env on any verb ---

#[test]
fn session_flag_beats_env_on_any_verb() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    fx.git(&["branch", "other"]);
    fx.write("a.txt", "changed\n");

    // FF_SESSION=a, but --session b — the switch pre-snapshot should stamp b.
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["switch", "--session", "b", "other"])
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
        .env("FF_SESSION", "a")
        .output()
        .expect("spawn ff");
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let repo = fx.path();
    let snap_ref = read_ref(&repo, "refs/fufu/snap/main");
    let msg = git_cat_file_commit(&repo, &snap_ref);
    assert!(msg.contains("fufu-session: b"), "flag value stamped: {msg}");
    assert!(
        !msg.contains("fufu-session: a"),
        "env value should not appear: {msg}"
    );
}

// --- bad --session flag errors before work ---

#[test]
fn bad_session_flag_errors_before_work() {
    let fx = Fixture::new();
    fx.write("a.txt", "initial\n");
    fx.commit("init");

    // Empty session name — should exit 2 with usage/bad-session.
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["status", "--session", "", "--json"])
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
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff");
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code 2 for empty session: stderr={}",
        stderr(&out)
    );
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-session");
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
