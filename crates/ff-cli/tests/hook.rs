//! The `ff trigger` runtime contract.
//!
//! Two halves, and they are opposites. A client source always exits 0,
//! prints nothing but the briefing and whatever `fufu.gitPolicy` had to say
//! about raw git, and swallows every failure. It never vetoes on its own
//! judgment, and the one veto config can ask for travels as JSON rather
//! than as an exit code. The `manual` source is a verb like any other:
//! loud, `--json` capable, and an error outside a repository.
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

/// Get this session's briefing over with, so a test about something else
/// is reading only that. The briefing rides `PreToolUse` now — it has to,
/// because that is the only channel that reaches a subagent — so a test
/// that wants a silent tool call has to have been briefed already.
///
/// The warm-up takes its own snapshot, so the tree is re-dirtied after it:
/// an unmoved tree captures to `NoOp` and the subject under test would
/// never land.
fn brief_first(fx: &Fixture, source: &str, session: &str) {
    let body = payload("UserPromptSubmit", session, &fx.path(), r#""prompt":"hi""#);
    let out = ff_stdin(&fx.path(), &["trigger", source], &body);
    assert!(
        !out.stdout.is_empty(),
        "the warm-up is what briefs: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    dirty(fx, "dirty again\n");
}

// ---- the raw-git correction ------------------------------------------------

/// A Bash payload carrying a raw git command.
fn git_payload(session: &str, cwd: &Path, command: &str) -> String {
    payload(
        "PreToolUse",
        session,
        cwd,
        &format!(
            r#""tool_name":"Bash","tool_input":{{"command":{}}}"#,
            serde_json::to_string(command).unwrap()
        ),
    )
}

/// Observe records and says nothing at all — byte-identical to a
/// PreToolUse that had nothing to say.
#[test]
fn observe_is_silent() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "observe");
    brief_first(&fx, "claude", "s");
    let body = git_payload("s", &fx.path(), "git commit -m x");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "observe says nothing: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(git commit -m x)");
}

/// Coach injects the alternative as context and decides no permission at
/// all — `"allow"` would suppress the user's own prompt for a tool fufu was
/// only commenting on.
#[test]
fn coach_injects_context_without_deciding_permission() {
    let fx = repo();
    let body = git_payload("s", &fx.path(), "git commit -m x");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid json");
    let hook = &value["hookSpecificOutput"];
    assert_eq!(hook["hookEventName"], "PreToolUse");
    assert!(
        hook["additionalContext"]
            .as_str()
            .is_some_and(|t| t.contains("ff commit")),
        "coach names the verb: {text}"
    );
    assert!(
        hook.get("permissionDecision").is_none(),
        "coach must not decide permission: {text}"
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(git commit -m x)");

    // The same word again in the same session says nothing more.
    dirty(&fx, "again\n");
    let again = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(again.status.code(), Some(0));
    assert!(
        again.stdout.is_empty(),
        "coach names each word once per session: {}",
        String::from_utf8_lossy(&again.stdout)
    );
}

/// Strict denies, carrying the verb to run instead — and still exits 0,
/// because a denial is JSON the client may ignore, never an exit code.
#[test]
fn strict_denies_and_still_exits_zero() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "strict");
    let body = git_payload("s", &fx.path(), "git commit -m x");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid json");
    let hook = &value["hookSpecificOutput"];
    assert_eq!(hook["permissionDecision"], "deny");
    assert!(
        hook["permissionDecisionReason"]
            .as_str()
            .is_some_and(|t| t.contains("ff commit")),
        "the denial names the verb: {text}"
    );
    // The capture is never conditional on the correction.
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(git commit -m x)");
}

/// A command fufu cannot read as one plain git invocation fails open: no
/// denial, under strict, and the snapshot lands anyway.
#[test]
fn a_compound_command_is_never_denied() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let body = git_payload("s", &fx.path(), "make && git commit -m x");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "ambiguity fails open: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        chain_subject(&fx),
        "claude[s]: Bash(make && git commit -m x)"
    );
}

/// A write with no fufu verb to name is nobody's to correct, under any tier.
#[test]
fn a_write_fufu_cannot_answer_passes_under_strict() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let body = git_payload("s", &fx.path(), "git apply p.diff");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "git apply has no fufu answer: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(git apply p.diff)");
}

// ---- the client sources ----------------------------------------------------

/// The first tool call in a repository briefs, because `PreToolUse` is
/// the only channel that reaches a subagent or a directory the agent has
/// just entered — and this repository's marker has never been written.
#[test]
fn pretooluse_bash_snapshots_with_provenance_and_briefs_the_first_time() {
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
    let text = String::from_utf8(out.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid json");
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .is_some_and(|t| t.starts_with("fufu (`ff`) is capturing")),
        "a tool call is a channel the briefing can reach: {text}"
    );
    assert!(out.stderr.is_empty());
    assert_eq!(
        chain_subject(&fx),
        "claude[01234567]: Bash(rm -rf build && make)"
    );

    // The second call of the same session says nothing more.
    dirty(&fx, "again\n");
    let again = ff_stdin(elsewhere.path(), &["trigger", "claude"], &body);
    assert!(
        again.stdout.is_empty(),
        "an audience is briefed once: {}",
        String::from_utf8_lossy(&again.stdout)
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
        /// Whether this client has a documented channel on a tool call.
        /// Only Claude Code does, and that is what decides whether the
        /// briefing can ride one here.
        speaks_on_a_tool: bool,
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
            speaks_on_a_tool: true,
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
            speaks_on_a_tool: false,
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
            speaks_on_a_tool: false,
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
            speaks_on_a_tool: false,
        },
    ];

    for vendor in cases {
        let fx = repo();
        let body = (vendor.payload)(&fx.path());
        let source = vendor.source;
        let out = ff_stdin(&fx.path(), &["trigger", source], &body);
        assert_eq!(out.status.code(), Some(0), "{source} exits 0");
        assert_eq!(chain_subject(&fx), vendor.subject, "{source} provenance");

        // Three of the four have no channel on a tool, so nothing is said
        // there — and the marker is proof of the rule that makes that
        // safe: it is stamped only when something actually printed, so a
        // briefing that had nowhere to go is not recorded as delivered.
        let marker = fx.path().join(".git/fufu/session").join(source);
        if vendor.speaks_on_a_tool {
            assert!(
                !out.stdout.is_empty(),
                "{source} carries the briefing on a tool"
            );
            assert!(marker.exists(), "{source} recorded the audience it briefed");
        } else {
            assert!(out.stdout.is_empty(), "{source} writes no stdout on a tool");
            assert!(
                !marker.exists(),
                "{source} printed nothing, so it recorded nothing"
            );
        }
    }
}

/// The four capture-only events. Nothing is injected on any of them —
/// `reply_envelope` has no channel for those kinds and the reply is empty
/// anyway — and each one lands the snapshot that is the whole reason it is
/// wired. `Stop` and `SubagentStop` are what make the last edit of a turn
/// durable: capture is snapshot-*before*, so without them the file state an
/// agent writes as its final action waits for whatever comes next.
#[test]
fn the_turn_end_events_capture_and_say_nothing() {
    for (event, subject) in [
        ("Stop", "claude[s]: event Stop"),
        ("SubagentStop", "claude[s]: event SubagentStop"),
        ("SubagentStart", "claude[s]: event SubagentStart"),
        ("CwdChanged", "claude[s]: event CwdChanged"),
    ] {
        let fx = repo();
        let body = payload(event, "s", &fx.path(), "");
        let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
        assert_eq!(out.status.code(), Some(0), "{event} exits 0");
        assert!(
            out.stdout.is_empty(),
            "{event} is a capture lane: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(out.stderr.is_empty(), "{event} says nothing");
        assert_eq!(chain_subject(&fx), subject, "{event} lands a snapshot");
    }
}

/// `SubagentStop` carries no `cwd`, which used to make every one of them a
/// silent error. The process directory is the honest fallback: the client
/// spawns the hook in the session's own directory.
#[test]
fn a_payload_with_no_cwd_still_captures() {
    let fx = repo();
    let body = r#"{"hook_event_name":"SubagentStop","session_id":"s","agent_id":"sub-1"}"#;
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], body);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert_eq!(chain_subject(&fx), "claude[s]: event SubagentStop");
}

/// A context boundary — a startup, a resume, a `/clear`, a fork, a
/// compaction — hands back the same session id, and the briefing injected
/// into the context it replaced is gone with it. So `SessionStart` re-briefs
/// unconditionally, where a second turn on that id does not.
#[test]
fn a_session_start_rebriefs_where_a_second_turn_does_not() {
    let fx = repo();
    let turn = |session: &str| {
        let body = payload("UserPromptSubmit", session, &fx.path(), r#""prompt":"hi""#);
        ff_stdin(&fx.path(), &["trigger", "claude"], &body)
    };

    assert!(!turn("s1").stdout.is_empty(), "the first turn briefs");
    assert!(turn("s1").stdout.is_empty(), "the second does not");

    let body = payload("SessionStart", "s1", &fx.path(), r#""source":"compact""#);
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with("fufu (`ff`) is capturing"),
        "a boundary briefs on an id the marker already holds: {text:?}"
    );
    assert!(
        turn("s1").stdout.is_empty(),
        "and the turn after it is quiet again"
    );
}

/// A subagent inherits the parent's session id, fires no prompt event, and
/// was told nothing — so it is an audience of its own, reached on its first
/// write-tool call and once only.
#[test]
fn a_subagent_is_briefed_once_on_its_own_first_tool_call() {
    let fx = repo();
    let call = |agent: &str| {
        let body = payload(
            "PreToolUse",
            "s1",
            &fx.path(),
            &format!(r#""agent_id":"{agent}","tool_name":"Bash","tool_input":{{"command":"ls"}}"#),
        );
        let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
        dirty(&fx, &format!("after {agent}\n"));
        String::from_utf8(out.stdout).unwrap()
    };

    let first = call("sub-1");
    let value: serde_json::Value = serde_json::from_str(first.trim()).expect("valid json");
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .is_some_and(|t| t.starts_with("fufu (`ff`) is capturing")),
        "the subagent's first write is where fufu can reach it: {first}"
    );
    assert!(call("sub-1").is_empty(), "and it hears it once");
    assert!(
        !call("sub-2").is_empty(),
        "a second subagent is a second context"
    );

    // The main thread is its own audience, and this one has been briefed —
    // the subagents' copies were not its.
    let body = payload("UserPromptSubmit", "s1", &fx.path(), r#""prompt":"hi""#);
    assert!(
        !ff_stdin(&fx.path(), &["trigger", "claude"], &body)
            .stdout
            .is_empty(),
        "the main thread was never told"
    );
    dirty(&fx, "after the prompt\n");
    let body = payload(
        "PreToolUse",
        "s1",
        &fx.path(),
        r#""tool_name":"Bash","tool_input":{"command":"ls"}"#,
    );
    assert!(
        ff_stdin(&fx.path(), &["trigger", "claude"], &body)
            .stdout
            .is_empty(),
        "and a tool call with no agent id is that same audience"
    );
}

/// Claude Code parses a hook's stdout as a *single* JSON object, and the
/// briefing and a refusal can now both fall due on one `PreToolUse`. Two
/// prints would lose both.
#[test]
fn a_briefing_and_a_denial_arrive_as_one_object() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "strict");
    let body = git_payload("s", &fx.path(), "git commit -m x");
    let out = ff_stdin(&fx.path(), &["trigger", "claude"], &body);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.trim().lines().count(),
        1,
        "one object, not two: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid json");
    let hook = &value["hookSpecificOutput"];
    assert!(
        hook["additionalContext"]
            .as_str()
            .is_some_and(|t| t.starts_with("fufu (`ff`) is capturing")),
        "the briefing rode it: {text}"
    );
    assert_eq!(hook["permissionDecision"], "deny");
    assert!(
        hook["permissionDecisionReason"]
            .as_str()
            .is_some_and(|t| t.contains("ff commit")),
        "and so did the refusal: {text}"
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(git commit -m x)");
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
    let marker_session = |slug: &str| {
        let body = std::fs::read(session_dir.join(slug)).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        value["session"].as_str().unwrap().to_string()
    };
    assert_eq!(marker_session("claude"), "claude-1");
    assert_eq!(marker_session("codex"), "codex-1");

    // A fresh session re-briefs, and only that client's marker moves.
    assert!(
        !brief("claude", "UserPromptSubmit", "claude-2")
            .stdout
            .is_empty()
    );
    assert_eq!(marker_session("codex"), "codex-1");
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
    brief_first(&fx, "claude", "s");
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
