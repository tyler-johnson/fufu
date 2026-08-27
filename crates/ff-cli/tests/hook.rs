//! The `ff trigger` runtime contract.
//!
//! Two halves, and they are opposites. A client source always exits 0,
//! never vetoes, prints nothing but the briefing, and swallows every
//! failure. The `manual` source is a verb like any other: loud, `--json`
//! capable, and an error outside a repository.
//!
//! The adapter-parity proof is one recorded payload per vendor landing a
//! snapshot with that vendor's own name on it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

/// A HOME no test may escape. Every runner here pins it, because a trigger
/// that fell through to the installer would otherwise rewrite the config of
/// whoever is running the suite.
fn scratch_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::TempDir::new().expect("scratch HOME"))
        .path()
}

/// Run `ff` with a payload on stdin.
fn ff_stdin(cwd: &Path, args: &[&str], payload: &str) -> Output {
    ff_stdin_home(cwd, args, payload, scratch_home())
}

/// The same, against a HOME of the caller's own. The briefing reads what is
/// installed there, so a test about the skill cannot share the scratch one.
fn ff_stdin_home(cwd: &Path, args: &[&str], payload: &str, home: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff");
    // An oversized payload makes ff stop reading and exit early; the
    // resulting broken pipe on our side is expected.
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    child.wait_with_output().expect("wait ff")
}

fn ff(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", scratch_home())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

/// JSON-encode a path for splicing into a payload — Windows backslashes must
/// arrive escaped, exactly as the clients send them.
fn json_path(path: &Path) -> String {
    serde_json::to_string(&path.display().to_string()).unwrap()
}

fn payload(event: &str, session: &str, cwd: &Path, extra: &str) -> String {
    format!(
        r#"{{"hook_event_name":"{event}","session_id":"{session}","cwd":{}{}{extra}}}"#,
        json_path(cwd),
        if extra.is_empty() { "" } else { "," }
    )
}

fn chain_subject(fx: &Fixture) -> String {
    fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"])
        .trim()
        .to_string()
}

fn dirty(fx: &Fixture, text: &str) {
    fx.write("a.txt", text);
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    fx
}

// ---- the client sources ----------------------------------------------------

#[test]
fn pretooluse_bash_snapshots_with_provenance_and_no_output() {
    let fx = repo();
    let body = payload(
        "PreToolUse",
        "0123456789abcdef",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"rm -rf build && make"}"#,
    );
    // cwd comes from the payload: run from elsewhere entirely.
    let elsewhere = tempfile::TempDir::new().unwrap();
    let out = ff_stdin(elsewhere.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "PreToolUse never writes stdout");
    assert!(out.stderr.is_empty());
    assert_eq!(
        chain_subject(&fx),
        "claude[01234567]: Bash(rm -rf build && make)"
    );
}

#[test]
fn edit_provenance_uses_a_path_relative_to_the_worktree() {
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
            r#""tool_name":"Edit","tool_input":{{"file_path":{}}}"#,
            json_path(&abs)
        ),
    );
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(chain_subject(&fx), "claude[sess]: Edit(src/lib.rs)");
}

#[test]
fn unknown_tool_is_labeled_honestly_and_snapshots_anyway() {
    let fx = repo();
    let body = payload(
        "PreToolUse",
        "sess",
        &fx.path(),
        r#""tool_name":"FutureTool","tool_input":{}"#,
    );
    assert_eq!(
        ff_stdin(&fx.path(), &["trigger", "claude"], &body)
            .status
            .code(),
        Some(0)
    );
    assert_eq!(chain_subject(&fx), "claude[sess]: tool FutureTool");
}

#[test]
fn unknown_event_snapshots_with_an_honest_label() {
    let fx = repo();
    let body = payload(
        "SomethingTheyAddedLater",
        "sess",
        &fx.path(),
        r#""tool_name":"Bash""#,
    );
    assert_eq!(
        ff_stdin(&fx.path(), &["trigger", "claude"], &body)
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        chain_subject(&fx),
        "claude[sess]: event SomethingTheyAddedLater"
    );
}

/// The adapter-parity proof: one recorded payload per vendor, each landing
/// a snapshot under its own name. Everything after the parse is shared, so
/// this is the whole of what an adapter is responsible for.
#[test]
fn every_vendor_lands_a_snapshot_under_its_own_name() {
    /// A recorded payload for one vendor: its source name, how it spells
    /// the payload, and the subject that must land.
    struct Vendor {
        source: &'static str,
        payload: fn(&Path) -> String,
        subject: &'static str,
    }

    let cases = [
        Vendor {
            source: "claude",
            payload: |cwd| {
                payload(
                    "PreToolUse",
                    "s1",
                    cwd,
                    r#""tool_name":"Bash","tool_input":{"command":"cargo test"}"#,
                )
            },
            subject: "claude[s1]: Bash(cargo test)",
        },
        Vendor {
            source: "codex",
            payload: |cwd| {
                payload(
                    "PreToolUse",
                    "s2",
                    cwd,
                    r#""tool_name":"Bash","tool_input":{"command":"cargo test"}"#,
                )
            },
            subject: "codex[s2]: Bash(cargo test)",
        },
        Vendor {
            source: "gemini",
            payload: |cwd| {
                payload(
                    "BeforeTool",
                    "s3",
                    cwd,
                    r#""tool_name":"run_shell_command","tool_input":{"command":"cargo test"}"#,
                )
            },
            subject: "gemini[s3]: run_shell_command(cargo test)",
        },
        Vendor {
            source: "cursor",
            payload: |cwd| {
                format!(
                    r#"{{"hook_event_name":"preToolUse","conversation_id":"s4","cwd":{},"tool_name":"Shell","tool_input":{{"command":"cargo test"}}}}"#,
                    json_path(cwd)
                )
            },
            subject: "cursor[s4]: Shell(cargo test)",
        },
    ];

    for vendor in cases {
        let fx = repo();
        let body = (vendor.payload)(&fx.path());
        let source = vendor.source;
        let out = ff_stdin(&fx.path(), &["trigger", source], &body);
        assert_eq!(out.status.code(), Some(0), "{source} exits 0");
        assert!(out.stdout.is_empty(), "{source} writes no stdout on a tool");
        assert_eq!(chain_subject(&fx), vendor.subject, "{source} provenance");
    }
}

/// The briefing is one text and four envelopes: plain for Claude and Codex,
/// JSON for Gemini and Cursor, which cannot read anything else.
#[test]
fn the_briefing_is_wrapped_the_way_each_client_reads_it() {
    for (source, event, plain) in [
        ("claude", "UserPromptSubmit", true),
        ("codex", "UserPromptSubmit", true),
        ("gemini", "SessionStart", false),
        ("cursor", "sessionStart", false),
    ] {
        let fx = repo();
        let body = payload(event, "s", &fx.path(), r#""prompt":"hi""#);
        let out = ff_stdin(&fx.path(), &["trigger", source], &body);
        assert_eq!(out.status.code(), Some(0));
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.is_empty(), "{source} briefs on {event}");
        if plain {
            assert!(
                text.starts_with("fufu (`ff`) is capturing"),
                "{source} takes plain text: {text:?}"
            );
        } else {
            let value: serde_json::Value = serde_json::from_str(text.trim())
                .unwrap_or_else(|err| panic!("{source} must emit JSON ({err}): {text:?}"));
            let field = if source == "gemini" {
                value["hookSpecificOutput"]["additionalContext"].clone()
            } else {
                value["additional_context"].clone()
            };
            assert!(
                field.as_str().is_some_and(|t| t.contains("ff restore")),
                "{source} carries the briefing in its own field: {value}"
            );
        }
    }
}

/// The briefing marker is per-slug. Two clients in one repository must each
/// be briefed exactly once — with one shared marker they would clobber each
/// other's session id and re-brief forever.
#[test]
fn two_clients_in_one_repo_are_each_briefed_once() {
    let fx = repo();
    let brief = |source: &str, event: &str, session: &str| {
        let body = payload(event, session, &fx.path(), r#""prompt":"hi""#);
        ff_stdin(&fx.path(), &["trigger", source], &body)
    };

    assert!(
        !brief("claude", "UserPromptSubmit", "claude-1")
            .stdout
            .is_empty()
    );
    assert!(
        !brief("codex", "UserPromptSubmit", "codex-1")
            .stdout
            .is_empty()
    );
    // The second turn of either session is silent, and stays silent with
    // the other client interleaved.
    assert!(
        brief("claude", "UserPromptSubmit", "claude-1")
            .stdout
            .is_empty()
    );
    assert!(
        brief("codex", "UserPromptSubmit", "codex-1")
            .stdout
            .is_empty()
    );
    assert!(
        brief("claude", "UserPromptSubmit", "claude-1")
            .stdout
            .is_empty()
    );

    let session_dir = fx.path().join(".git/fufu/session");
    assert_eq!(
        std::fs::read_to_string(session_dir.join("claude")).unwrap(),
        "claude-1"
    );
    assert_eq!(
        std::fs::read_to_string(session_dir.join("codex")).unwrap(),
        "codex-1"
    );

    // A fresh session re-briefs, and only that client's marker moves.
    assert!(
        !brief("claude", "UserPromptSubmit", "claude-2")
            .stdout
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(session_dir.join("codex")).unwrap(),
        "codex-1"
    );
}

#[test]
fn the_briefing_teaches_only_live_spellings() {
    let fx = repo();
    let body = payload(
        "UserPromptSubmit",
        "session-aaa",
        &fx.path(),
        r#""prompt":"please fix the tests""#,
    );
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    let notice = String::from_utf8(out.stdout).unwrap();
    assert!(
        notice.contains("ff log") && notice.contains("ff restore"),
        "the notice teaches the agent the verbs: {notice:?}"
    );
    // The notice is the always-on spelling lesson: a retired or mistyped
    // form there teaches the agent to fail. Guard the two that already went
    // wrong. The positive half of the second — that an id goes to --at-op —
    // moved to the skill with the verb, and is guarded there.
    assert!(
        !notice.contains("ff -m"),
        "bare -m is retired; the notice must not teach it: {notice:?}"
    );
    assert!(
        !notice.contains("--at <id>"),
        "an id never goes to --at; --at takes a time: {notice:?}"
    );
    assert_eq!(
        chain_subject(&fx),
        "claude[session-]: prompt \"please fix the tests\""
    );
}

/// The briefing names the skill only where the skill actually landed. A
/// machine with no plugin — or one on the `--settings` escape hatch — must
/// not be told to read a manual that is not there.
#[test]
fn the_briefing_names_the_skill_only_where_it_is_installed() {
    let fx = repo();
    let home = tempfile::TempDir::new().unwrap();
    let body = payload("UserPromptSubmit", "s-1", &fx.path(), r#""prompt":"hi""#);

    let bare = String::from_utf8(
        ff_stdin_home(&fx.path(), &["trigger", "claude"], &body, home.path()).stdout,
    )
    .unwrap();
    assert!(bare.starts_with("fufu (`ff`) is capturing"), "{bare:?}");
    assert!(
        !bare.contains("skill"),
        "nothing is installed, so nothing is named: {bare:?}"
    );

    // Install the plugin, and brief a fresh session so the marker does not
    // suppress it.
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "claude"])
        .current_dir(fx.path())
        .env("HOME", home.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff");
    assert!(out.status.success());

    let body = payload("UserPromptSubmit", "s-2", &fx.path(), r#""prompt":"hi""#);
    let briefed = String::from_utf8(
        ff_stdin_home(&fx.path(), &["trigger", "claude"], &body, home.path()).stdout,
    )
    .unwrap();
    assert!(
        briefed.contains("`fufu` skill"),
        "the skill is on disk, so the briefing points at it: {briefed:?}"
    );
}

#[test]
fn malformed_and_hostile_payloads_exit_zero_silently() {
    let fx = repo();
    let outside = tempfile::TempDir::new().unwrap();
    let cases: Vec<String> = vec![
        String::new(),            // empty stdin
        "not json at all".into(), // garbage
        r#"{"cwd": 42}"#.into(),  // wrong type
        r#"{}"#.into(),           // no cwd
        payload(
            "PreToolUse",
            "s",
            outside.path(),
            r#""tool_name":"Bash","tool_input":{}"#,
        ), // outside a repo
        format!(
            r#"{{"hook_event_name":"PreToolUse","cwd":{},"padding":"{}"}}"#,
            json_path(&fx.path()),
            "x".repeat(9 * 1024 * 1024)
        ), // oversized
    ];
    for case in cases {
        let out = ff_stdin(&fx.path(), &["trigger", "claude"], &case);
        assert_eq!(
            out.status.code(),
            Some(0),
            "a trigger must never veto: {:?}",
            &case[..case.len().min(60)]
        );
        assert!(out.stdout.is_empty(), "no output on failure");
        assert!(out.stderr.is_empty(), "silent without FF_DEBUG");
    }
}

// ---- name resolution -------------------------------------------------------

/// `<vendor>-<event>` forces the event from the name, overriding whatever
/// the payload's own field says. That is what a client whose payload cannot
/// identify its own event needs.
#[test]
fn a_vendor_event_name_overrides_the_payload() {
    let fx = repo();
    // The payload says PreToolUse; the name says the turn ended.
    let body = payload(
        "PreToolUse",
        "sess",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
    );
    let out = ff_stdin(&fx.path(), &["trigger", "claude-posttooluse"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "not a context start: no briefing");
    // The event was forced, so the label is the event's rather than the
    // tool's — the name won.
    assert_eq!(chain_subject(&fx), "claude[sess]: event PreToolUse");
}

/// An unknown source is the published extension point: exit 0, say nothing,
/// capture nothing. That is what makes a fufu trigger safe to wire into a
/// client fufu has never heard of.
#[test]
fn an_unknown_source_exits_zero_without_output() {
    let fx = repo();
    let body = payload("PreToolUse", "s", &fx.path(), r#""tool_name":"Bash""#);
    for source in ["notaclient", "notaclient-pretooluse", "manual-x"] {
        let out = ff_stdin(&fx.path(), &["trigger", source], &body);
        assert_eq!(out.status.code(), Some(0), "{source} exits 0");
        assert!(out.stdout.is_empty(), "{source} says nothing");
        assert!(out.stderr.is_empty(), "{source} says nothing");
    }
    // Nothing was captured at all: the chain does not exist.
    let out = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "refs/fufu/snap/main"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "an unknown source captures nothing: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ---- the manual source -----------------------------------------------------

#[test]
fn bare_trigger_and_manual_are_the_same_snapshot() {
    for args in [vec!["trigger"], vec!["trigger", "manual"]] {
        let fx = repo();
        let out = ff(&fx.path(), &args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert_eq!(chain_subject(&fx), "manual", "{args:?}");
    }
}

#[test]
fn a_label_lands_in_the_subject() {
    let fx = repo();
    let out = ff(&fx.path(), &["trigger", "-m", "before the risky bit"]);
    assert!(out.status.success());
    assert_eq!(chain_subject(&fx), "manual: before the risky bit");
}

/// The two contracts, side by side: the same failure is loud for the manual
/// source and silent for a client one.
#[test]
fn the_manual_source_is_loud_where_a_client_source_is_silent() {
    let outside = tempfile::TempDir::new().unwrap();

    let out = ff(outside.path(), &["trigger"]);
    assert_ne!(out.status.code(), Some(0), "manual errors outside a repo");
    assert!(!out.stderr.is_empty(), "and says so");

    let body = payload("PreToolUse", "s", outside.path(), r#""tool_name":"Bash""#);
    let out = ff_stdin(outside.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0), "a client source never vetoes");
    assert!(out.stderr.is_empty(), "and never complains");
}

#[test]
fn json_is_an_envelope_for_manual_and_ignored_for_a_client() {
    let fx = repo();
    let out = ff(&fx.path(), &["--json", "trigger", "-m", "checkpoint"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("one envelope on stdout");
    assert_eq!(value["cmd"], "trigger");
    assert_eq!(value["data"]["source"], "manual");
    assert_eq!(value["data"]["captured"], true);
    assert!(value["data"]["op"].is_string());

    // A client source owns its stream: `--json` must not put an envelope
    // on it, because the briefing is what the client is reading there.
    dirty(&fx, "dirtier\n");
    let body = payload(
        "PreToolUse",
        "s",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
    );
    let out = ff_stdin(&fx.path(), &["--json", "trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "no envelope on a client's stream");
}

#[test]
fn a_second_manual_snapshot_of_an_unmoved_tree_is_not_news() {
    let fx = repo();
    assert!(ff(&fx.path(), &["trigger"]).status.success());
    let out = ff(&fx.path(), &["trigger"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("already snapshotted"),
        "says what happened: {text:?}"
    );
}

// ---- legacy spellings ------------------------------------------------------

/// Every trigger spelling ever stored in a config file still captures. A
/// stale entry must never become a silent capture outage.
#[test]
fn every_shipped_trigger_spelling_still_captures() {
    for spelling in [
        vec!["trigger", "claude"],
        vec!["hook", "agent", "trigger", "claude"],
        vec!["hook", "claude"],
    ] {
        let fx = repo();
        let body = payload(
            "PreToolUse",
            "legacysess",
            &fx.path(),
            r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
        );
        let out = ff_stdin(&fx.path(), &spelling, &body);
        assert_eq!(out.status.code(), Some(0), "{spelling:?}");
        assert_eq!(
            chain_subject(&fx),
            "claude[legacyse]: Bash(ls)",
            "{spelling:?} must still capture"
        );
    }
}

/// The rc-file spelling, which is not a vendor name at all.
#[test]
fn the_legacy_shell_trigger_spelling_still_resolves() {
    let fx = repo();
    // stdout is a pipe here, so the ambient channel's first gate fires and
    // it does nothing — the point is that the name resolves and exits 0
    // rather than falling through to the unknown-name path.
    for spelling in [vec!["trigger", "shell"], vec!["hook", "shell", "trigger"]] {
        let out = ff(&fx.path(), &spelling);
        assert_eq!(out.status.code(), Some(0), "{spelling:?}");
        assert!(out.stdout.is_empty(), "{spelling:?}");
    }
}

/// The landmine: `ff hook claude` used to mean *trigger* and now means
/// *install*. With a payload on stdin it is the trigger; without one it is
/// the installer, and installing never reads stdin.
#[test]
fn the_hook_claude_shim_routes_by_whether_stdin_holds_a_payload() {
    let fx = repo();
    let home = tempfile::TempDir::new().unwrap();

    // With a payload: the trigger. Nothing is installed.
    let body = payload(
        "PreToolUse",
        "shim",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "claude"])
        .current_dir(fx.path())
        .env("HOME", home.path())
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
    assert_eq!(chain_subject(&fx), "claude[shim]: Bash(ls)");
    assert!(
        !home.path().join(".claude/skills/fufu").exists(),
        "the trigger path installs nothing"
    );

    // Without one: the installer.
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["hook", "claude"])
        .current_dir(fx.path())
        .env("HOME", home.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        home.path()
            .join(".claude/skills/fufu/hooks/hooks.json")
            .exists(),
        "the install path wrote the plugin"
    );
}
