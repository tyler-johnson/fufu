//! The `ff trigger` runtime contract.
//!
//! Two halves, and they are opposites. A client source always exits 0,
//! prints nothing but the briefing, whatever `fufu.gitPolicy` had to say
//! about raw git, and whatever `fufu.toolPolicy` had to say about `ff` in
//! the shell, and swallows every failure. It never vetoes on its own
//! judgment, and the two vetoes config can ask for travel as JSON rather
//! than as an exit code. The `manual` source is a verb like any other:
//! loud, `--json` capable, and an error outside a repository.
//!
//! The adapter-parity proof is one recorded payload per vendor landing a
//! snapshot with that vendor's own name on it.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use ff_testsupport::userdirs;

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
    ff_stdin_with(cwd, args, payload, home, &[])
}

/// The same, with variables of the caller's own on top. Both user roots are
/// pinned under `home` on every platform, so the tool steer's presence
/// marker and the extension registry resolve inside the scratch home and
/// never the real one, and `CLAUDE_PID` is removed unless a test sets it —
/// the suite itself may be running under a Claude that set one.
fn ff_stdin_with(
    cwd: &Path,
    args: &[&str],
    payload: &str,
    home: &Path,
    envs: &[(&str, &str)],
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.args(args).current_dir(cwd);
    userdirs::pin(&mut cmd, home)
        .env_remove("CLAUDE_PID")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("spawn ff");
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
    bash_payload(session, cwd, command)
}

/// A Bash payload carrying a command.
fn bash_payload(session: &str, cwd: &Path, command: &str) -> String {
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

// ---- the tool steer --------------------------------------------------------

/// A Bash payload carrying an `ff` command.
fn ff_payload(session: &str, cwd: &Path, command: &str) -> String {
    bash_payload(session, cwd, command)
}

/// `<cache>/fufu/mcp/<pid>/`, as the binary resolves it under the scratch
/// HOME: a directory per client, holding a marker per server name.
fn marker_dir(pid: u32) -> std::path::PathBuf {
    userdirs::cache_root(scratch_home())
        .join("fufu")
        .join("mcp")
        .join(pid.to_string())
}

/// fufu's own marker, held the way a live server holds it: exclusively
/// locked. The file is returned so the test keeps the lock for as long as
/// it wants.
fn serving_marker(pid: u32) -> File {
    named_marker(pid, "fufu")
}

/// The same under a server name of the caller's own, which is what a
/// declared extension's server would hold if it held one.
fn named_marker(pid: u32, name: &str) -> File {
    std::fs::create_dir_all(marker_dir(pid)).unwrap();
    let file = File::create(marker_dir(pid).join(name)).unwrap();
    file.try_lock().expect("the test holds the marker");
    file
}

/// A marker a server left behind: the file, and nobody holding it.
fn stale_marker(pid: u32) -> std::path::PathBuf {
    std::fs::create_dir_all(marker_dir(pid)).unwrap();
    let path = marker_dir(pid).join("fufu");
    std::fs::write(&path, "{\"server\":1}\n").unwrap();
    path
}

/// One steered trigger, as Claude Code would fire it: `CLAUDE_PID` set to
/// `client`, or absent when `None`.
fn steered(fx: &Fixture, session: &str, command: &str, client: Option<u32>) -> Output {
    let body = ff_payload(session, &fx.path(), command);
    let pid = client.map(|pid| pid.to_string());
    let envs: Vec<(&str, &str)> = pid.iter().map(|pid| ("CLAUDE_PID", pid.as_str())).collect();
    ff_stdin_with(
        &fx.path(),
        &["trigger", "claude"],
        &body,
        scratch_home(),
        &envs,
    )
}

fn hook_output(out: &Output) -> serde_json::Value {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("valid json");
    value["hookSpecificOutput"].clone()
}

/// Strict denies `ff` in the shell while a server is up for the calling
/// client, names the tool and the exact call, and still exits 0 with the
/// capture landed.
#[test]
fn strict_denies_ff_in_the_shell_and_still_exits_zero() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    let _held = serving_marker(4242);
    let out = steered(&fx, "s", "ff status", Some(4242));
    assert_eq!(out.status.code(), Some(0));
    let hook = hook_output(&out);
    assert_eq!(hook["permissionDecision"], "deny", "{hook}");
    let reason = hook["permissionDecisionReason"].as_str().expect("a reason");
    assert!(reason.contains("mcp__plugin_fufu_fufu__ff"), "{reason}");
    assert!(reason.contains(r#"{"args":["status"]}"#), "{reason}");
    assert!(reason.contains("fufu.toolPolicy is strict"), "{reason}");
    // The capture is never conditional on the steer.
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(ff status)");
}

/// The setting's default is strict: an unset repository refuses too.
#[test]
fn the_default_tool_policy_is_strict() {
    let fx = repo();
    let _held = serving_marker(4242);
    let out = steered(&fx, "s", "ff log -n 3", Some(4242));
    assert_eq!(out.status.code(), Some(0));
    let hook = hook_output(&out);
    assert_eq!(hook["permissionDecision"], "deny", "{hook}");
    assert!(
        hook["permissionDecisionReason"]
            .as_str()
            .is_some_and(|t| t.contains(r#"{"args":["log","-n","3"]}"#)),
        "{hook}"
    );
}

/// No marker on disk means no server is up, and nothing is said.
#[test]
fn no_server_means_no_refusal() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let _ = std::fs::remove_dir_all(marker_dir(4343));
    let out = steered(&fx, "s", "ff status", Some(4343));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "nothing is up, so nothing is said: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A marker nobody holds is a server that died. The first reader sweeps it
/// and says nothing.
#[test]
fn a_stale_marker_is_swept_and_refuses_nothing() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let path = stale_marker(4444);
    let out = steered(&fx, "s", "ff status", Some(4444));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "a stale marker refuses nothing: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(!path.exists(), "and the hook swept it");
}

/// The marker is keyed by client: another Claude's server is not this one's.
#[test]
fn another_clients_marker_does_not_count() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(4545);
    let out = steered(&fx, "s", "ff status", Some(4546));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "a different client's server: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A client that did not say who it is cannot be matched to a server, and
/// the fail-open direction is silence.
#[test]
fn without_claude_pid_nothing_is_said() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(4646);
    let out = steered(&fx, "s", "ff status", None);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "no client pid, no steer: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// The verbs the tool does not offer belong in the shell, so they pass.
#[test]
fn the_shell_only_verbs_pass_the_steer() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(4747);
    let out = steered(&fx, "s", "ff git push", Some(4747));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "ff git is shell-only: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(ff git push)");
}

/// Declare one extension on the machine the steered tests run against, by
/// writing the registry `ff extension add` would have written.
///
/// Only a word that is neither shell-only nor a builtin verb reads it, so
/// the tests that steer `ff status` and `ff git push` are untouched by
/// whatever is on it.
fn declare(name: &str) {
    declare_undoable(name, true);
}

/// The same, saying whether the extension's writes can be taken back. The
/// tool will not serve one that answers no, so the shell keeps it.
fn declare_undoable(name: &str, undoable: bool) {
    let file = userdirs::registry(scratch_home());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let body = serde_json::json!({
        "ff": 1,
        "extensions": [{
            "path": format!("/usr/local/bin/ff-{name}"),
            "declared_at": 1788462398,
            "manifest": {
                "name": name,
                "version": "0.4.1",
                "contract": 1,
                "verbs": [{"name": "brief", "read_only": true}],
                "undoable": undoable,
            },
        }],
    });
    std::fs::write(&file, body.to_string()).unwrap();
}

/// A declared extension is served through the tool, so the refusal fires
/// for it too, and names it so the agent can tell it from the undeclared
/// one beside it.
#[test]
fn a_declared_extension_is_refused_and_named() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    declare("tower");
    let _held = serving_marker(5252);
    let out = steered(&fx, "s", "ff tower brief 65", Some(5252));
    assert_eq!(out.status.code(), Some(0));
    let hook = hook_output(&out);
    assert_eq!(hook["permissionDecision"], "deny", "{hook}");
    let reason = hook["permissionDecisionReason"].as_str().expect("a reason");
    assert!(reason.contains("tower is a declared extension"), "{reason}");
    assert!(reason.contains("mcp__plugin_fufu_fufu__ff"), "{reason}");
    assert!(
        reason.contains(r#"{"args":["tower","brief","65"]}"#),
        "{reason}"
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(ff tower brief 65)");
}

/// An extension nobody declared passes, because a shell is the only place
/// one runs: the tool does not serve it, and refusing it here would leave
/// a strict repository nowhere to run it.
#[test]
fn an_undeclared_extension_passes_the_steer() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(5353);
    let out = steered(&fx, "s", "ff nothing-declared-this run", Some(5353));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "nobody declared it, so the shell is where it runs: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        chain_subject(&fx),
        "claude[s]: Bash(ff nothing-declared-this run)"
    );
}

/// The marker is keyed by the server's name as well as the client's pid,
/// and fufu's own is the only one the steer asks about: a marker held under
/// some other server's name is not fufu's tool being up.
#[test]
fn another_servers_marker_does_not_count() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    declare("tower");
    brief_first(&fx, "claude", "s");
    let _held = named_marker(5454, "tower");
    let out = steered(&fx, "s", "ff status", Some(5454));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "fufu's own server is not up: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A declared extension whose writes are not undoable is the shell's, and
/// its own server being up changes nothing: the tool will not serve it, so
/// refusing here would leave it nowhere to run.
#[test]
fn an_extensions_own_server_does_not_refuse_what_the_tool_will_not_serve() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    declare_undoable("tower", false);
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(5555);
    let _theirs = named_marker(5555, "tower");
    let out = steered(&fx, "s", "ff tower brief 65", Some(5555));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "the shell is the only place it runs: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(chain_subject(&fx), "claude[s]: Bash(ff tower brief 65)");
}

/// Unlike the git lane, a compound command is read per segment: the `ff`
/// segment is read with certainty, and `cwd` is the tool's answer to the
/// `cd` in front of it.
#[test]
fn a_compound_command_is_refused_by_its_ff_segment() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "strict");
    let _held = serving_marker(4848);
    let out = steered(&fx, "s", "cd sub && ff status", Some(4848));
    assert_eq!(out.status.code(), Some(0));
    let hook = hook_output(&out);
    assert_eq!(hook["permissionDecision"], "deny", "{hook}");
    assert!(
        hook["permissionDecisionReason"]
            .as_str()
            .is_some_and(|t| t.contains(r#"{"args":["status"]}"#)),
        "{hook}"
    );
}

/// Observe says nothing, with the server up and the shell reaching for ff.
#[test]
fn observe_says_nothing_about_the_tool() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "observe");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(4949);
    let out = steered(&fx, "s", "ff status", Some(4949));
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "observe is silent: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Coach names the tool once per session as context, decides no
/// permission, and starts over in a new session.
#[test]
fn coach_names_the_tool_once_per_session() {
    let fx = repo();
    fx.set_config("fufu.toolPolicy", "coach");
    brief_first(&fx, "claude", "s");
    let _held = serving_marker(5050);

    let first = steered(&fx, "s", "ff status", Some(5050));
    assert_eq!(first.status.code(), Some(0));
    let hook = hook_output(&first);
    assert!(
        hook["additionalContext"].as_str().is_some_and(
            |t| t.contains("the ff tool is up") && t.contains(r#"{"args":["status"]}"#)
        ),
        "coach names the tool: {hook}"
    );
    assert!(
        hook.get("permissionDecision").is_none(),
        "coach must not decide permission: {hook}"
    );

    let again = steered(&fx, "s", "ff log", Some(5050));
    assert_eq!(again.status.code(), Some(0));
    assert!(
        again.stdout.is_empty(),
        "once per session: {}",
        String::from_utf8_lossy(&again.stdout)
    );

    let next = steered(&fx, "t", "ff log", Some(5050));
    assert_eq!(next.status.code(), Some(0));
    let hook = hook_output(&next);
    assert!(
        hook["additionalContext"].as_str().is_some_and(|t| t
            .starts_with("fufu (`ff`) is capturing")
            && t.contains("the ff tool is up")),
        "a new session is briefed and coached again, in one object: {hook}"
    );
    assert!(hook.get("permissionDecision").is_none(), "{hook}");
}

/// The git lane fails open on a compound command, so on one line carrying
/// both a raw git write and an `ff`, the ff segment is what gets refused.
/// The two refusals cannot coincide today — the git lane needs a plain
/// `git …` and nothing else — and the reply carries one reason regardless.
#[test]
fn one_reply_carries_one_reason() {
    let fx = repo();
    fx.set_config("fufu.gitPolicy", "strict");
    fx.set_config("fufu.toolPolicy", "strict");
    let _held = serving_marker(5151);
    let out = steered(&fx, "s", "git commit -m x; ff status", Some(5151));
    assert_eq!(out.status.code(), Some(0));
    let hook = hook_output(&out);
    assert_eq!(hook["permissionDecision"], "deny", "{hook}");
    let reason = hook["permissionDecisionReason"].as_str().expect("a reason");
    assert!(reason.contains("fufu.toolPolicy"), "{reason}");
    assert!(!reason.contains("fufu.gitPolicy"), "{reason}");
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

// ---- a declared extension's briefing line ----------------------------------

/// One line per declared extension rides fufu's own briefing: static text
/// from the manifest, or whatever `ff-<name> briefing` prints at print time.
/// It rides the same boundaries and the same once-per-audience marker, and
/// every way an extension can fail to produce one costs the briefing
/// nothing.
///
/// Unix only, for the reason `tests/ext.rs` is: these run a real binary, and
/// a shell script is the smallest one to write. PATH is pinned to the test's
/// own bin directory rather than prepended, the landmine
/// `tests/extension.rs` documents — this machine may have a real `ff-tower`
/// installed, and the registry these write is pinned under the test's HOME
/// for the same reason.
#[cfg(unix)]
mod declared_extensions {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use super::*;

    /// A machine: a HOME with its own config root, and a directory that is
    /// the whole of PATH.
    pub(super) fn machine() -> (tempfile::TempDir, tempfile::TempDir) {
        (
            tempfile::TempDir::new().expect("create home"),
            tempfile::TempDir::new().expect("create bin dir"),
        )
    }

    /// An `ff-<name>` whose body is whatever the test wants. Every run
    /// appends its argv and the three variables to a note beside it, so a
    /// test can say whether the binary ran at all and what it was handed.
    fn ext_bin(bin: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = bin.join(format!("ff-{name}"));
        let note = note_path(bin, name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$1\" \"$FF_CONTRACT\" \"$FF_REPO\" \
                 \"$(pwd -P)\" >> {}\n{body}\n",
                note.display()
            ),
        )
        .expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    pub(super) fn note_path(bin: &Path, name: &str) -> std::path::PathBuf {
        bin.join(format!("{name}.ran"))
    }

    /// What `ff-<name>` recorded about the runs it was given, one per line.
    pub(super) fn ran(bin: &Path, name: &str) -> Vec<String> {
        std::fs::read_to_string(note_path(bin, name))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// A manifest of the smallest shape, with `briefing` spelled as the
    /// caller wrote it — a JSON string, `true`, or nothing at all.
    fn manifest(name: &str, briefing: Option<&str>) -> String {
        let briefing = briefing
            .map(|value| format!(r#","briefing":{value}"#))
            .unwrap_or_default();
        format!(
            r#"{{"name":"{name}","version":"0.1.0","contract":1,
                "verbs":[{{"name":"board","read_only":true}}],"undoable":true{briefing}}}"#
        )
    }

    /// Write the registry this machine reads, declaring the named
    /// extensions in the order given.
    pub(super) fn declare(home: &Path, bin: &Path, manifests: &[String]) {
        let records: Vec<String> = manifests
            .iter()
            .map(|body| {
                let manifest: serde_json::Value =
                    serde_json::from_str(body).expect("the manifest is json");
                let name = manifest["name"].as_str().expect("a name");
                serde_json::json!({
                    "path": bin.join(format!("ff-{name}")),
                    "declared_at": 1_788_462_398_i64,
                    "manifest": manifest,
                })
                .to_string()
            })
            .collect();
        let file = userdirs::registry(home);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("create config dir");
        std::fs::write(
            &file,
            format!(r#"{{"ff":1,"extensions":[{}]}}"#, records.join(",")),
        )
        .expect("write registry");
    }

    /// Brief one audience: a turn boundary in `session`, with PATH pinned to
    /// `bin`. Answers what the client was told.
    fn brief(fx: &Fixture, home: &Path, bin: &Path, session: &str) -> String {
        let body = payload("UserPromptSubmit", session, &fx.path(), r#""prompt":"hi""#);
        let out = ff_stdin_with(
            &fx.path(),
            &["trigger", "claude"],
            &body,
            home,
            &[("PATH", &bin.display().to_string())],
        );
        assert_eq!(out.status.code(), Some(0), "a trigger always exits 0");
        String::from_utf8(out.stdout).expect("utf-8 stdout")
    }

    /// The notice is always the whole of what fufu itself says, whatever
    /// else rode with it.
    pub(super) fn briefed(text: &str) {
        assert!(
            text.starts_with("fufu (`ff`) is capturing"),
            "the notice leads: {text:?}"
        );
        assert!(
            text.contains("Every verb's own `--help` is the authority on it."),
            "and it is all there: {text:?}"
        );
    }

    /// The static arm: the line is in the manifest, so nothing runs. Two
    /// extensions come out in the order they were declared, which is what
    /// the registry being a list is for.
    #[test]
    fn a_static_line_rides_the_briefing_in_declaration_order() {
        let fx = repo();
        let (home, bin) = machine();
        ext_bin(bin.path(), "tower", "exit 0");
        ext_bin(bin.path(), "bay", "exit 0");
        declare(
            home.path(),
            bin.path(),
            &[
                manifest("tower", Some(r#""Work is filed as flights on a board.""#)),
                manifest("bay", Some(r#""Bays are warmed before they are used.""#)),
            ],
        );

        let text = brief(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        let tower = text.find("Work is filed").expect("tower's line");
        let bay = text.find("Bays are warmed").expect("bay's line");
        assert!(tower < bay, "declaration order is the order: {text:?}");
        assert!(
            tower > text.find("--help is the authority").unwrap_or(0),
            "fufu speaks first: {text:?}"
        );
        assert!(
            ran(bin.path(), "tower").is_empty() && ran(bin.path(), "bay").is_empty(),
            "a line the manifest already carries runs nothing"
        );
    }

    /// The asked arm: `true` in the manifest means run `ff-<name> briefing`
    /// at print time, in the event's own directory and with the three
    /// variables set.
    #[test]
    fn an_asked_line_is_produced_at_print_time() {
        let fx = repo();
        let (home, bin) = machine();
        ext_bin(
            bin.path(),
            "tower",
            r#"if [ "$1" = "briefing" ]; then echo "tower: two flights are in progress here."; fi"#,
        );
        declare(home.path(), bin.path(), &[manifest("tower", Some("true"))]);

        let text = brief(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        assert!(
            text.contains("tower: two flights are in progress here."),
            "the binary's line rode it: {text:?}"
        );

        // The verb, the contract, and a directory inside the worktree — the
        // same handshake an extension is handed anywhere else.
        let runs = ran(bin.path(), "tower");
        assert_eq!(runs.len(), 1, "asked once: {runs:?}");
        let fields: Vec<&str> = runs[0].split('|').collect();
        assert_eq!(fields[0], "briefing");
        assert_eq!(fields[1], "1", "FF_CONTRACT: {runs:?}");
        let here = std::fs::canonicalize(fx.path()).expect("canonical");
        let leaf = here.file_name().expect("leaf");
        assert_eq!(
            Path::new(fields[2]).file_name(),
            Some(leaf),
            "FF_REPO is the worktree: {runs:?}"
        );
        assert_eq!(
            Path::new(fields[3]).file_name(),
            Some(leaf),
            "and it ran in the event's own directory: {runs:?}"
        );
    }

    /// The cap. A line past it is dropped whole rather than cut, and the
    /// briefing is exactly what it would have been.
    #[test]
    fn a_line_past_the_cap_costs_the_briefing_nothing() {
        let fx = repo();
        let (home, bin) = machine();
        ext_bin(bin.path(), "tower", "exit 0");
        let long = "flights ".repeat(60);
        declare(
            home.path(),
            bin.path(),
            &[manifest("tower", Some(&format!(r#""{long}""#)))],
        );

        let text = brief(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        assert!(!text.contains("flights"), "the long line went: {text:?}");
    }

    /// Every way an extension can fail to produce a line is the same
    /// answer, and it is the answer a machine with nothing declared gets.
    #[test]
    fn a_binary_that_fails_costs_the_briefing_nothing() {
        let bodies = [
            // Exited nonzero, having printed a line anyway.
            r#"echo "tower: a line"; exit 1"#,
            // Exited 0 with nothing to say, and with only whitespace.
            "exit 0",
            "echo",
            // Several lines, which is not one line.
            r#"echo "tower: a line"; echo "tower: another""#,
        ];
        for body in bodies {
            let fx = repo();
            let (home, bin) = machine();
            ext_bin(bin.path(), "tower", body);
            declare(home.path(), bin.path(), &[manifest("tower", Some("true"))]);

            let text = brief(&fx, home.path(), bin.path(), "s-1");
            briefed(&text);
            assert!(!text.contains("tower:"), "{body}: {text:?}");
        }
    }

    /// A record outliving its binary costs a `None` and nothing else.
    #[test]
    fn a_binary_gone_from_path_costs_the_briefing_nothing() {
        let fx = repo();
        let (home, bin) = machine();
        declare(home.path(), bin.path(), &[manifest("tower", Some("true"))]);

        let text = brief(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
    }

    /// The time box: a binary that never answers is a binary the briefing
    /// waits one budget for and then goes on without.
    #[test]
    fn a_binary_that_hangs_costs_the_briefing_nothing() {
        let fx = repo();
        let (home, bin) = machine();
        // PATH is the test's own bin directory, so the script names a
        // system one of its own to reach `sleep`. Anything the script does
        // to its environment stays inside it.
        ext_bin(
            bin.path(),
            "tower",
            r#"echo "tower: a line"; PATH=/bin:/usr/bin; export PATH; sleep 120"#,
        );
        declare(home.path(), bin.path(), &[manifest("tower", Some("true"))]);

        let started = Instant::now();
        let text = brief(&fx, home.path(), bin.path(), "s-1");
        let waited = started.elapsed();
        briefed(&text);
        assert!(!text.contains("tower:"), "it never answered: {text:?}");
        assert!(
            waited >= Duration::from_millis(500),
            "the box was never reached, so this proved nothing: waited {waited:?}"
        );
        assert!(
            waited.as_secs() < 30,
            "the box is fufu's, not the extension's: waited {waited:?}"
        );
    }

    /// An extension's line is briefing, so it is paid for once per
    /// audience, exactly as the notice is.
    #[test]
    fn an_extension_line_is_briefed_once_per_audience() {
        let fx = repo();
        let (home, bin) = machine();
        ext_bin(
            bin.path(),
            "tower",
            r#"if [ "$1" = "briefing" ]; then echo "tower: two flights."; fi"#,
        );
        declare(home.path(), bin.path(), &[manifest("tower", Some("true"))]);

        let first = brief(&fx, home.path(), bin.path(), "s-1");
        assert!(first.contains("tower: two flights."), "{first:?}");
        dirty(&fx, "dirty again\n");

        let again = brief(&fx, home.path(), bin.path(), "s-1");
        assert!(again.is_empty(), "the audience was already told: {again:?}");
        assert_eq!(
            ran(bin.path(), "tower").len(),
            1,
            "and a briefing that is not due asks nobody"
        );

        // A fresh session is a fresh context, so everyone hears it again.
        dirty(&fx, "dirty once more\n");
        let fresh = brief(&fx, home.path(), bin.path(), "s-2");
        assert!(fresh.contains("tower: two flights."), "{fresh:?}");
    }

    /// The notice's own budget is not something an extension can spend.
    /// Whatever rides with it, what fufu says is unchanged.
    #[test]
    fn the_notice_is_unchanged_with_extensions_declared() {
        let fx = repo();
        let (home, bin) = machine();
        ext_bin(bin.path(), "tower", "exit 0");
        declare(
            home.path(),
            bin.path(),
            &[manifest("tower", Some(r#""Work is filed as flights.""#))],
        );
        let with = brief(&fx, home.path(), bin.path(), "s-1");

        let bare_fx = repo();
        let (bare_home, bare_bin) = machine();
        let bare = brief(&bare_fx, bare_home.path(), bare_bin.path(), "s-1");

        assert_eq!(
            with.replace("Work is filed as flights.", "").trim_end(),
            bare.trim_end(),
            "the extension added a line and changed nothing else"
        );
    }
}

// ---- the fan-out to subscribed extensions ----------------------------------

/// The neutral event, handed to every declared extension whose manifest
/// named its kind: `ff-<name> trigger`, the event as one JSON object on
/// stdin, and one field read back out of the reply. What a subscriber says
/// is merged into the reply the client was already getting — fufu's own
/// lines first, then each subscriber in the order it was declared.
///
/// Unix only, and PATH pinned to the test's own bin directory rather than
/// prepended, for the reasons `declared_extensions` gives. The fixtures are
/// that module's, since this is the same machine seen from a second angle.
#[cfg(unix)]
mod subscribed_extensions {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use super::declared_extensions::{briefed, declare, machine, note_path, ran};
    use super::*;

    /// An `ff-<name>` that records the verb it was called with and the event
    /// it was handed, one run per line, and then does whatever the test
    /// wants. `read` is the shell's own builtin, because PATH here is the
    /// test's bin directory and nothing else.
    fn sub_bin(bin: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = bin.join(format!("ff-{name}"));
        let note = note_path(bin, name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nread -r said\nprintf '%s %s\\n' \"$1\" \"$said\" >> {}\n{body}\n",
                note.display()
            ),
        )
        .expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// A manifest of the smallest shape, subscribed to the events the caller
    /// spelled.
    fn manifest(name: &str, events: &str) -> String {
        format!(
            r#"{{"name":"{name}","version":"0.1.0","contract":1,
                "verbs":[{{"name":"board","read_only":true}}],"undoable":true,
                "events":[{events}]}}"#
        )
    }

    /// A body that answers with the envelope a served extension prints.
    fn says(name: &str, context: &str) -> String {
        format!(r#"printf '{{"ff":1,"cmd":"{name} trigger","data":{{"context":"{context}"}}}}\n'"#)
    }

    /// Fire one client event with PATH pinned to `bin`, and answer with what
    /// the client was told.
    fn fire(
        fx: &Fixture,
        home: &Path,
        bin: &Path,
        event: &str,
        session: &str,
        extra: &str,
    ) -> String {
        let body = payload(event, session, &fx.path(), extra);
        let out = ff_stdin_with(
            &fx.path(),
            &["trigger", "claude"],
            &body,
            home,
            &[("PATH", &bin.display().to_string())],
        );
        assert_eq!(out.status.code(), Some(0), "a trigger always exits 0");
        String::from_utf8(out.stdout).expect("utf-8 stdout")
    }

    /// A turn boundary, which is the kind that carries the briefing too.
    fn turn(fx: &Fixture, home: &Path, bin: &Path, session: &str) -> String {
        fire(
            fx,
            home,
            bin,
            "UserPromptSubmit",
            session,
            r#""prompt":"hi""#,
        )
    }

    /// The event one subscriber was handed, as JSON.
    fn saw(bin: &Path, name: &str) -> Vec<serde_json::Value> {
        ran(bin, name)
            .iter()
            .map(|line| {
                let (verb, wire) = line.split_once(' ').expect("a verb and an event");
                assert_eq!(verb, "trigger", "the verb is always the same one");
                serde_json::from_str(wire)
                    .unwrap_or_else(|err| panic!("one json object: {err}: {wire}"))
            })
            .collect()
    }

    /// The whole of it: the event goes out on stdin, and the one field fufu
    /// reads out of the reply comes back in the reply the client already
    /// gets, after fufu's own lines.
    #[test]
    fn a_subscriber_is_handed_the_event_and_its_context_lands_in_the_reply() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(
            bin.path(),
            "tower",
            &says("tower", "tower: flight 73 is in progress here."),
        );
        declare(
            home.path(),
            bin.path(),
            &[manifest("tower", r#"{"kind":"ContextStart"}"#)],
        );

        let text = turn(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        let said = text
            .find("tower: flight 73 is in progress here.")
            .expect("the subscriber's context rode the reply");
        assert!(
            text.find("--help is the authority").unwrap_or(0) < said,
            "fufu speaks first: {text:?}"
        );

        let events = saw(bin.path(), "tower");
        assert_eq!(events.len(), 1, "asked once: {events:?}");
        let event = &events[0];
        assert_eq!(event["ff"], 1);
        assert_eq!(event["kind"], "ContextStart");
        assert_eq!(event["source"], "claude");
        assert_eq!(event["session"], "s-1");
        assert_eq!(event["agent"], "");
        assert_eq!(
            Path::new(event["cwd"].as_str().expect("a cwd")).file_name(),
            std::fs::canonicalize(fx.path())
                .expect("canonical")
                .file_name(),
            "the event's own directory: {event}"
        );
        assert!(
            event["label"]
                .as_str()
                .expect("a label")
                .starts_with("prompt "),
            "the subject's detail: {event}"
        );
        // Every field is present and the ones that do not apply are null, so
        // a handler indexes rather than probes.
        for absent in ["tool", "command", "path"] {
            assert!(event[absent].is_null(), "{absent} is null here: {event}");
        }
        assert_eq!(
            event.as_object().expect("an object").len(),
            10,
            "ten fields, always: {event}"
        );
    }

    /// The two fields the fan-out adds. A tool call carries the tool's name
    /// and the file it named, so a subscriber acts on the call rather than
    /// parsing the subject line.
    #[test]
    fn a_tool_call_carries_the_tool_and_the_file_it_named() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", "exit 0");
        declare(
            home.path(),
            bin.path(),
            &[manifest(
                "tower",
                r#"{"kind":"BeforeTool","matcher":"Edit"}"#,
            )],
        );

        let file = fx.path().join("a.txt");
        fire(
            &fx,
            home.path(),
            bin.path(),
            "PreToolUse",
            "s-1",
            &format!(
                r#""tool_name":"Edit","tool_input":{{"file_path":{}}}"#,
                json_path(&file)
            ),
        );

        let events = saw(bin.path(), "tower");
        assert_eq!(events.len(), 1, "one tool call, one spawn: {events:?}");
        assert_eq!(events[0]["kind"], "BeforeTool");
        assert_eq!(events[0]["tool"], "Edit");
        assert_eq!(
            events[0]["path"], "a.txt",
            "relative to the worktree: {}",
            events[0]
        );
        assert!(events[0]["command"].is_null(), "an edit runs nothing");
    }

    /// Two subscribers are merged in the order they were declared, which is
    /// what the registry being a list is for.
    #[test]
    fn two_subscribers_merge_in_registry_order() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", &says("tower", "tower: a board."));
        sub_bin(bin.path(), "bay", &says("bay", "bay: a warm worktree."));
        declare(
            home.path(),
            bin.path(),
            &[
                manifest("tower", r#"{"kind":"ContextStart"}"#),
                manifest("bay", r#"{"kind":"ContextStart"}"#),
            ],
        );

        let text = turn(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        let tower = text.find("tower: a board.").expect("tower spoke");
        let bay = text.find("bay: a warm worktree.").expect("bay spoke");
        assert!(tower < bay, "declaration order is the order: {text:?}");
    }

    /// An extension that subscribed to nothing is never spawned. Declaring
    /// buys being described; being woken is what a subscription buys.
    #[test]
    fn an_extension_that_subscribed_to_nothing_is_never_spawned() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", &says("tower", "tower: a board."));
        declare(home.path(), bin.path(), &[manifest("tower", "")]);

        let text = turn(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        assert!(!text.contains("tower:"), "it never ran: {text:?}");
        assert!(ran(bin.path(), "tower").is_empty(), "nothing was spawned");
    }

    /// A subscription is per kind, so an event of another kind spawns
    /// nobody — which is what keeps a `TurnEnd` subscriber off the tool
    /// call's critical path.
    #[test]
    fn an_unsubscribed_kind_spawns_nobody() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", &says("tower", "tower: a board."));
        declare(
            home.path(),
            bin.path(),
            &[manifest("tower", r#"{"kind":"TurnEnd"}"#)],
        );

        let text = turn(&fx, home.path(), bin.path(), "s-1");
        briefed(&text);
        assert!(!text.contains("tower:"), "wrong kind: {text:?}");
        assert!(ran(bin.path(), "tower").is_empty(), "nothing was spawned");

        // And the kind it did subscribe to reaches it.
        dirty(&fx, "dirty again\n");
        fire(&fx, home.path(), bin.path(), "Stop", "s-1", "");
        assert_eq!(saw(bin.path(), "tower").len(), 1, "TurnEnd is its kind");
    }

    /// A `BeforeTool` subscription is per tool as well as per kind: the
    /// matcher is the tool names it wants, matched whole and by case, and a
    /// tool call it did not name spawns nobody. That is what keeps a spawn
    /// per tool call proportionate to what the extension is for.
    #[test]
    fn a_matcher_decides_which_tool_calls_wake_a_subscriber() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", "exit 0");
        declare(
            home.path(),
            bin.path(),
            &[manifest(
                "tower",
                r#"{"kind":"BeforeTool","matcher":"Edit|Write"}"#,
            )],
        );

        let call = |tool: &str| {
            fire(
                &fx,
                home.path(),
                bin.path(),
                "PreToolUse",
                "s-1",
                &format!(r#""tool_name":"{tool}","tool_input":{{"command":"ls"}}"#),
            );
        };

        // A tool the matcher does not name, and one it only spells part of:
        // a name matches whole, so `Edit` is not `NotebookEdit`, and case is
        // the client's.
        for tool in ["Bash", "NotebookEdit", "edit", "Edit|Write"] {
            call(tool);
            assert!(
                ran(bin.path(), "tower").is_empty(),
                "{tool} is not a tool it asked for"
            );
        }

        call("Write");
        let events = saw(bin.path(), "tower");
        assert_eq!(events.len(), 1, "the second name it wants: {events:?}");
        assert_eq!(events[0]["tool"], "Write");
    }

    /// An event carrying no tool name matches nothing, so a shell prompt
    /// spawns no `BeforeTool` subscriber even though the kind fufu gives it
    /// is `BeforeTool`. The subscriber is on the tool call's critical path,
    /// and this is what keeps it off everything else's.
    #[test]
    fn an_event_with_no_tool_name_wakes_no_before_tool_subscriber() {
        let fx = repo();
        let (home, bin) = machine();
        sub_bin(bin.path(), "tower", &says("tower", "tower: a board."));
        declare(
            home.path(),
            bin.path(),
            &[manifest(
                "tower",
                r#"{"kind":"BeforeTool","matcher":"Edit|Write"}"#,
            )],
        );

        let out = ff_stdin_with(
            &fx.path(),
            &["trigger", "shell"],
            "",
            home.path(),
            &[("PATH", &bin.path().display().to_string())],
        );
        assert_eq!(out.status.code(), Some(0), "a trigger always exits 0");
        assert!(
            ran(bin.path(), "tower").is_empty(),
            "a prompt names no tool"
        );
    }

    /// Every way a subscriber can fail to say something is one answer, and
    /// it is the answer a machine with nothing subscribed gets. stdout is
    /// read as one envelope on one line and nothing else.
    #[test]
    fn a_failing_subscriber_costs_the_reply_nothing() {
        let bodies = [
            // Exited nonzero, having printed a perfectly good envelope.
            r#"printf '{"ff":1,"data":{"context":"tower: a line."}}\n'; exit 1"#,
            // Exited 0 with nothing to say.
            "exit 0",
            // Not an envelope at all.
            r#"echo "tower: a line.""#,
            // A banner in front of one.
            r#"echo "ff-tower 0.1.0"; printf '{"ff":1,"data":{"context":"tower: a line."}}\n'"#,
            // An envelope reporting a failure is a subscriber with nothing
            // to say, not a failure fufu reports.
            r#"printf '{"ff":1,"cmd":"tower trigger","error":{"message":"tower: a line."}}\n'"#,
        ];
        for body in bodies {
            let fx = repo();
            let (home, bin) = machine();
            sub_bin(bin.path(), "tower", body);
            declare(
                home.path(),
                bin.path(),
                &[manifest("tower", r#"{"kind":"ContextStart"}"#)],
            );

            let text = turn(&fx, home.path(), bin.path(), "s-1");
            briefed(&text);
            assert!(!text.contains("tower: a line."), "{body}: {text:?}");
        }
    }

    /// A record outliving its binary costs a `None` and nothing else.
    #[test]
    fn a_subscriber_gone_from_path_costs_the_reply_nothing() {
        let fx = repo();
        let (home, bin) = machine();
        declare(
            home.path(),
            bin.path(),
            &[manifest("tower", r#"{"kind":"ContextStart"}"#)],
        );
        briefed(&turn(&fx, home.path(), bin.path(), "s-1"));
    }

    /// The box is fufu's and it is one box over the whole fan-out, split
    /// over the subscribers that have not been asked yet. A subscriber that
    /// never answers contributes nothing, the event carries on without it,
    /// and the extension declared after it is still heard: the registry's
    /// order decides who is asked first and must not also decide who is
    /// heard.
    #[test]
    fn a_hanging_subscriber_is_cut_at_the_budget_and_starves_nobody() {
        let fx = repo();
        let (home, bin) = machine();
        // PATH is the test's own bin directory, so the script names a system
        // one of its own to reach `sleep`. What it does to its environment
        // stays inside it.
        sub_bin(
            bin.path(),
            "tower",
            r#"PATH=/bin:/usr/bin; export PATH; sleep 120"#,
        );
        sub_bin(bin.path(), "bay", &says("bay", "bay: a warm worktree."));
        declare(
            home.path(),
            bin.path(),
            &[
                manifest("tower", r#"{"kind":"ContextStart"}"#),
                manifest("bay", r#"{"kind":"ContextStart"}"#),
            ],
        );

        let started = Instant::now();
        let text = turn(&fx, home.path(), bin.path(), "s-1");
        let waited = started.elapsed();
        briefed(&text);
        assert!(
            text.contains("bay: a warm worktree."),
            "the one after it was still heard: {text:?}"
        );
        assert_eq!(ran(bin.path(), "tower").len(), 1, "it did run");
        assert!(
            waited >= Duration::from_millis(200),
            "the box was never reached, so this proved nothing: waited {waited:?}"
        );
        assert!(
            waited.as_secs() < 30,
            "the box is fufu's, not the extension's: waited {waited:?}"
        );
    }
}
