//! The `ff hook claude` runtime contract: correct provenance per payload
//! shape, the once-per-session notice (marker-first), and absolute
//! never-fail/never-veto behavior on malformed input.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_hook(cwd: &Path, payload: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "agent", "trigger", "claude"])
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff hook");
    // An oversized payload makes ff stop reading and exit early; the
    // resulting broken pipe on our side is expected.
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    child.wait_with_output().expect("wait ff hook")
}

fn payload(event: &str, session: &str, cwd: &Path, extra: &str) -> String {
    format!(
        r#"{{"hook_event_name":"{event}","session_id":"{session}","cwd":"{}"{}{extra}}}"#,
        cwd.display(),
        if extra.is_empty() { "" } else { "," }
    )
}

fn chain_subject(fx: &Fixture) -> String {
    fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"])
        .trim()
        .to_string()
}

#[test]
fn pretooluse_bash_snapshots_with_provenance_and_no_output() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let body = payload(
        "PreToolUse",
        "0123456789abcdef",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"rm -rf build && make"}"#,
    );
    // cwd comes from the payload: run from elsewhere entirely.
    let elsewhere = tempfile::TempDir::new().unwrap();
    let out = ff_hook(elsewhere.path(), &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "PreToolUse never writes stdout");
    assert!(out.stderr.is_empty());
    assert_eq!(
        chain_subject(&fx),
        "claude[01234567]: Bash(rm -rf build && make)"
    );
}

#[test]
fn edit_provenance_uses_relative_path() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "fn main() {}\n");
    fx.commit("init");
    fx.write("src/lib.rs", "dirty\n");

    let abs = fx.path().join("src/lib.rs");
    let body = payload(
        "PreToolUse",
        "sess",
        &fx.path(),
        &format!(
            r#""tool_name":"Edit","tool_input":{{"file_path":"{}"}}"#,
            abs.display()
        ),
    );
    let out = ff_hook(&fx.path(), &body);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(chain_subject(&fx), "claude[sess]: Edit(src/lib.rs)");
}

#[test]
fn unknown_tool_is_labeled_honestly_and_snapshots_anyway() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let body = payload(
        "PreToolUse",
        "sess",
        &fx.path(),
        r#""tool_name":"FutureTool","tool_input":{}"#,
    );
    assert_eq!(ff_hook(&fx.path(), &body).status.code(), Some(0));
    assert_eq!(chain_subject(&fx), "claude[sess]: tool FutureTool");
}

#[test]
fn prompt_notice_once_per_session_marker_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let body = payload(
        "UserPromptSubmit",
        "session-aaa",
        &fx.path(),
        r#""prompt":"please fix the tests""#,
    );
    let out = ff_hook(&fx.path(), &body);
    assert_eq!(out.status.code(), Some(0));
    let notice = String::from_utf8(out.stdout).unwrap();
    assert!(
        notice.contains("ff log") && notice.contains("ff restore"),
        "the notice teaches the agent the verbs: {notice:?}"
    );
    let marker = fx.path().join(".git/fufu/claude-session");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "session-aaa");
    assert_eq!(
        chain_subject(&fx),
        "claude[session-]: prompt \"please fix the tests\""
    );

    // Same session again: silent.
    let out = ff_hook(&fx.path(), &body);
    assert!(out.stdout.is_empty(), "notice prints once per session");

    // New session: notice again.
    let body = payload(
        "UserPromptSubmit",
        "session-bbb",
        &fx.path(),
        r#""prompt":"hi""#,
    );
    let out = ff_hook(&fx.path(), &body);
    assert!(!out.stdout.is_empty(), "fresh session re-notifies");
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        "session-bbb",
        "marker follows the session"
    );
}

#[test]
fn malformed_and_hostile_payloads_exit_zero_silently() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let outside = tempfile::TempDir::new().unwrap();
    let cases: Vec<String> = vec![
        String::new(),            // empty stdin
        "not json at all".into(), // garbage
        r#"{"cwd": 42}"#.into(),  // wrong type: serde default rescues? no — type error
        r#"{}"#.into(),           // no cwd
        payload(
            "PreToolUse",
            "s",
            outside.path(),
            r#""tool_name":"Bash","tool_input":{}"#,
        ), // outside a repo
        format!(
            r#"{{"hook_event_name":"PreToolUse","cwd":"{}","padding":"{}"}}"#,
            fx.path().display(),
            "x".repeat(9 * 1024 * 1024)
        ), // oversized
    ];
    for case in cases {
        let out = ff_hook(&fx.path(), &case);
        assert_eq!(
            out.status.code(),
            Some(0),
            "a hook must never veto: {:?}",
            &case[..case.len().min(60)]
        );
        assert!(out.stdout.is_empty(), "no output on failure");
        assert!(out.stderr.is_empty(), "silent without FF_DEBUG");
    }
}

/// The committed Phase 1 spelling `ff hook claude` still triggers a capture:
/// a stale settings entry must never become a silent capture outage.
#[test]
fn legacy_trigger_spelling_still_captures() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let body = payload(
        "PreToolUse",
        "legacysess",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "claude"])
        .current_dir(fx.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(body.as_bytes());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(chain_subject(&fx), "claude[legacyse]: Bash(ls)");
}

#[test]
fn unknown_event_snapshots_with_honest_label() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let body = payload("PostToolUse", "sess", &fx.path(), r#""tool_name":"Bash""#);
    assert_eq!(ff_hook(&fx.path(), &body).status.code(), Some(0));
    assert_eq!(chain_subject(&fx), "claude[sess]: event PostToolUse");
}
