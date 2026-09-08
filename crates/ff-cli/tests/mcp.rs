//! `ff mcp`: the seven typed tools over stdio, driven by hand in both
//! protocol eras.
//!
//! Every test spawns the real binary as a server, writes JSON-RPC lines to
//! its stdin, and reads lines from its stdout. No client library, on
//! purpose: what a client sends is the contract, and a library would hide
//! which era's shape was being spoken.
//!
//! The user roots are pinned under a scratch HOME per test, since the
//! extension registry lives there and decides which produced tools are
//! served beside fufu's own.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use ff_testsupport::userdirs;
use serde_json::{Value, json};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// Responses that arrived while waiting for another id. Calls run
    /// concurrently, so answers may come back in any order.
    parked: HashMap<u64, Value>,
    /// The HOME and cache root this server was given, kept alive with it.
    home: tempfile::TempDir,
}

fn start(dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Server {
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    start_in(home, dir, extra, envs)
}

/// [`start`] under a HOME the test prepared first.
fn start_in(home: tempfile::TempDir, dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Server {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir).arg("mcp").args(extra);
    userdirs::pin(&mut cmd, home.path())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FF_DEBUG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn ff mcp");
    let stdin = child.stdin.take();
    let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    Server {
        child,
        stdin,
        stdout,
        parked: HashMap::new(),
        home,
    }
}

/// Declare one extension on the machine `home` stands for, the way
/// `ff extension add` records one. Unix only, since every test that
/// declares one then runs it as a shell script.
#[cfg(unix)]
fn declare(home: &Path, name: &str, verbs: &[&str], undoable: bool) {
    declare_promising(home, name, verbs, undoable, false);
}

/// [`declare`], with the manifest promising tools or not.
#[cfg(unix)]
fn declare_promising(home: &Path, name: &str, verbs: &[&str], undoable: bool, tools: bool) {
    let file = userdirs::registry(home);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("the config root");
    let verbs: Vec<Value> = verbs
        .iter()
        .map(|verb| json!({ "name": verb, "read_only": true }))
        .collect();
    let body = json!({
        "ff": 1,
        "extensions": [{
            "path": format!("/usr/local/bin/ff-{name}"),
            "declared_at": 1788462398,
            "manifest": {
                "name": name,
                "version": "0.4.1",
                "contract": 1,
                "verbs": verbs,
                "undoable": undoable,
                "tools": tools,
            },
        }],
    });
    std::fs::write(&file, body.to_string()).expect("write the registry");
}

impl Server {
    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{message}").expect("write a frame");
        stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method }));
    }

    /// One request, and its response — however many other responses
    /// arrive first.
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(&message);
        self.response(id)
    }

    fn response(&mut self, id: u64) -> Value {
        if let Some(parked) = self.parked.remove(&id) {
            return parked;
        }
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read a frame");
            assert!(read > 0, "the server closed stdout before answering {id}");
            let frame: Value = serde_json::from_str(line.trim()).expect("one JSON object per line");
            let got = frame["id"].as_u64();
            if got == Some(id) {
                return frame;
            }
            if let Some(got) = got {
                self.parked.insert(got, frame);
            }
        }
    }

    /// Close stdin and collect what the server did on the way out.
    fn close(self) -> (i32, String) {
        let (code, stderr, _home) = self.shutdown();
        (code, stderr)
    }

    /// The same, handing back the scratch HOME so a test can read it after
    /// the exit.
    fn shutdown(mut self) -> (i32, String, tempfile::TempDir) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait");
        let mut stderr = String::new();
        if let Some(mut err) = self.child.stderr.take() {
            std::io::Read::read_to_string(&mut err, &mut stderr).expect("read stderr");
        }
        (status.code().unwrap_or(-1), stderr, self.home)
    }
}

/// The legacy opening: `initialize`, then the `initialized` notification.
fn handshake(server: &mut Server) -> Value {
    let init = server.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }),
    );
    server.notify("notifications/initialized");
    init
}

/// One typed call, and its result.
fn call(server: &mut Server, id: u64, name: &str, arguments: Value) -> Value {
    server.request(
        id,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )["result"]
        .clone()
}

/// `ff` in a shell, against the fixture, for what a test needs to read or
/// arrange outside the tool.
fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FF_TOOL_CALL")
        .output()
        .expect("spawn ff")
}

/// The newest capture on the fixture's log, read from the shell.
fn newest_capture(dir: &Path) -> Value {
    let out = ff_at(dir, &["op", "log", "kind(capture)", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    log["data"]["ops"][0].clone()
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

const SEVEN: [&str; 7] = ["status", "pull", "push", "undo", "redo", "explain", "help"];

// ---- the legacy era --------------------------------------------------------

#[test]
fn the_legacy_handshake_lists_the_seven_tools_and_relays_the_envelope() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);

    let init = handshake(&mut server);
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["serverInfo"]["name"], "fufu");
    let instructions = init["result"]["instructions"]
        .as_str()
        .expect("the briefing is the instructions");
    assert!(
        instructions.starts_with("fufu (`ff`) is capturing"),
        "the briefing is the instructions: {init}"
    );
    assert!(
        instructions.contains("`fufu` tools"),
        "a server that is answering is offered by definition, so the instructions carry the \
         tools line: {init}"
    );

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, SEVEN, "the seven, in order: {listed}");
    for tool in tools {
        let name = &tool["name"];
        assert_eq!(
            tool["inputSchema"]["properties"]["cwd"]["type"], "string",
            "{name} takes cwd"
        );
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "{name} takes nothing else"
        );
        assert!(
            tool["annotations"]["readOnlyHint"].is_boolean()
                && tool["annotations"]["destructiveHint"].is_boolean(),
            "{name} states its hints: {tool}"
        );
    }
    assert_eq!(tools[2]["annotations"]["destructiveHint"], true, "push");

    // A reader: the envelope comes back whole, as text and as structure.
    let status = call(&mut server, 3, "status", json!({}));
    assert_ne!(status["isError"], true, "{status}");
    assert_eq!(status["structuredContent"]["ff"], 1);
    assert_eq!(status["structuredContent"]["cmd"], "status");
    assert_eq!(status["structuredContent"]["data"]["head"]["name"], "main");
    let text = status["content"][0]["text"].as_str().expect("text content");
    let parsed: Value = serde_json::from_str(text).expect("the text is the envelope");
    assert_eq!(parsed, status["structuredContent"]);
    // Twice: a reader repeats.
    let again = call(&mut server, 4, "status", json!({}));
    assert_eq!(again["structuredContent"]["data"]["head"]["name"], "main");

    // A fufu failure is a successful call carrying is_error and the id,
    // with the child's code in _meta.
    let missing = call(&mut server, 5, "explain", json!({ "id": "no/such-id" }));
    assert_eq!(missing["isError"], true, "{missing}");
    assert!(
        missing["structuredContent"]["error"]["id"].is_string(),
        "{missing}"
    );
    assert_eq!(missing["_meta"]["exit"], 2, "{missing}");

    // Help is text, and only text: the words after `ff help`, one per item.
    let help = call(&mut server, 6, "help", json!({ "verb": ["op", "log"] }));
    assert_ne!(help["isError"], true, "{help}");
    assert!(help.get("structuredContent").is_none(), "{help}");
    assert!(
        help["content"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("Usage: ff op log")),
        "{help}"
    );
    // And none of them is the map.
    let map = call(&mut server, 7, "help", json!({}));
    assert_ne!(map["isError"], true, "{map}");
    assert!(
        map["content"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("commit")),
        "{map}"
    );

    // A verb that is the shell is no tool: a protocol error, nothing ran.
    let git = server.request(
        8,
        "tools/call",
        json!({ "name": "git", "arguments": { "args": ["status"] } }),
    );
    assert!(git.get("error").is_some(), "{git}");
    assert!(
        git["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("status, pull, push, undo, redo, explain, help")),
        "{git}"
    );

    let (code, stderr, _home) = server.shutdown();
    assert_eq!(code, 0, "closing stdin ends the server cleanly");
    assert_eq!(stderr, "", "nothing on stderr without FF_DEBUG");
}

#[test]
fn cwd_runs_the_call_in_another_repository() {
    let here = repo();
    let there = Fixture::new();
    there.write("elsewhere.txt", "x\n");
    there.commit("elsewhere");
    let mut server = start(&here.path(), &[], &[]);
    handshake(&mut server);

    let there_path = there.path();
    let status = call(
        &mut server,
        2,
        "status",
        json!({ "cwd": there_path.to_str().unwrap() }),
    );
    assert_ne!(status["isError"], true, "{status}");
    assert_eq!(
        status["structuredContent"]["data"]["head"]["commit"],
        there.git(&["rev-parse", "HEAD"]).trim(),
        "{status}"
    );
    let (code, _) = server.close();
    assert_eq!(code, 0);
}

#[test]
fn malformed_input_is_a_protocol_error_not_a_tool_result() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    handshake(&mut server);

    // A number where a string belongs is a value a command line can spell,
    // so it reaches the child; a cwd that is not a string cannot.
    let bad_cwd = server.request(
        2,
        "tools/call",
        json!({ "name": "status", "arguments": { "cwd": 1 } }),
    );
    assert!(bad_cwd.get("error").is_some(), "{bad_cwd}");
    assert!(bad_cwd.get("result").is_none());

    let object = server.request(
        3,
        "tools/call",
        json!({ "name": "status", "arguments": { "at-op": { "id": 5 } } }),
    );
    assert!(object.get("error").is_some(), "{object}");

    let wrong_tool = server.request(
        4,
        "tools/call",
        json!({ "name": "git", "arguments": { "args": ["status"] } }),
    );
    assert!(wrong_tool.get("error").is_some(), "{wrong_tool}");

    // And a bad id where a string belongs is the child's to answer.
    let at_op = call(&mut server, 5, "status", json!({ "at-op": "5" }));
    assert_eq!(at_op["isError"], true, "{at_op}");
    assert!(
        at_op["structuredContent"]["error"]["id"].is_string(),
        "{at_op}"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// `--session` on the server tags every child's operations, which is how
/// an agent's work through the tool stays separable from a person's. Every
/// verb captures first, so a `status` over a dirty tree leaves a capture
/// carrying the session, and the shell reads it back.
#[test]
fn the_servers_session_rides_every_child() {
    let fx = repo();
    fx.write("b.txt", "b\n");
    let mut server = start(&fx.path(), &["--session", "flight-3"], &[]);
    handshake(&mut server);

    let status = call(&mut server, 2, "status", json!({}));
    assert_ne!(status["isError"], true, "{status}");

    let capture = newest_capture(&fx.path());
    assert!(
        capture["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("pre: ff") && s.ends_with("status --json")),
        "{capture}"
    );
    assert_eq!(capture["session"], "flight-3", "{capture}");
    assert_eq!(capture["route"], "tool", "{capture}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// `FF_SESSION` in the server's environment is the other spelling, and the
/// flag wins over it, the same precedence every invocation has.
#[test]
fn the_environment_session_is_read_and_the_flag_wins() {
    let fx = repo();
    fx.write("b.txt", "b\n");
    let mut server = start(&fx.path(), &[], &[("FF_SESSION", "from-env")]);
    handshake(&mut server);
    let status = call(&mut server, 2, "status", json!({}));
    assert_ne!(status["isError"], true, "{status}");
    server.close();
    assert_eq!(newest_capture(&fx.path())["session"], "from-env");

    fx.write("c.txt", "c\n");
    let mut server = start(
        &fx.path(),
        &["--session", "from-flag"],
        &[("FF_SESSION", "from-env")],
    );
    handshake(&mut server);
    let status = call(&mut server, 2, "status", json!({}));
    assert_ne!(status["isError"], true, "{status}");
    server.close();
    assert_eq!(newest_capture(&fx.path())["session"], "from-flag");
}

/// The server's children inherit the session `Ctx` settled for `ff mcp`,
/// which under a client that names one and no word of fufu's own is the
/// client's: a call through the tool carries the session the client's
/// hook captures do. And it says which road it took.
#[test]
fn the_servers_child_carries_the_clients_session_and_the_tool_route() {
    let fx = repo();
    fx.write("b.txt", "b\n");
    let mut server = start(
        &fx.path(),
        &[],
        &[(
            "CLAUDE_CODE_SESSION_ID",
            "95b36d9d-efdc-4564-9b06-91842f51ef6b",
        )],
    );
    handshake(&mut server);

    let status = call(&mut server, 2, "status", json!({}));
    assert_ne!(status["isError"], true, "{status}");

    let capture = newest_capture(&fx.path());
    assert!(
        capture["summary"]
            .as_str()
            .is_some_and(|s| s.starts_with("pre: ff") && s.ends_with("status --json")),
        "{capture}"
    );
    assert_eq!(
        capture["session"], "95b36d9d-efdc-4564-9b06-91842f51ef6b",
        "{capture}"
    );
    assert_eq!(capture["route"], "tool", "{capture}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

// ---- the modern era --------------------------------------------------------

/// 2026-07-28 has no handshake: the first request is `server/discover`,
/// every request carries its version in `_meta`, and the answer names
/// what the server speaks.
#[test]
fn the_modern_era_discovers_without_a_handshake() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "0" }
    });

    let discovered = server.request(1, "server/discover", json!({ "_meta": meta }));
    let versions = discovered["result"]["supportedVersions"]
        .as_array()
        .unwrap_or_else(|| panic!("supportedVersions: {discovered}"));
    assert!(versions.iter().any(|v| v == "2026-07-28"), "{discovered}");
    assert!(discovered["result"]["capabilities"].get("tools").is_some());

    let status = server.request(
        2,
        "tools/call",
        json!({ "_meta": meta, "name": "status", "arguments": {} }),
    );
    assert_eq!(status["result"]["resultType"], "complete", "{status}");
    assert_eq!(status["result"]["structuredContent"]["cmd"], "status");

    let (code, stderr) = server.close();
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
}

/// A client that opens the pipe and closes it again, which is how a client
/// probes whether a server starts, gets a clean exit and no complaint.
#[test]
fn closing_stdin_before_speaking_exits_zero() {
    let fx = repo();
    let server = start(&fx.path(), &[], &[]);
    let (code, stderr) = server.close();
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
}

// ---- extensions ------------------------------------------------------------

/// The server serves the tools a declared extension produced and nothing
/// for one nobody declared: no `tower__*` in the list, and a call on one is
/// a protocol error, since no child ever ran.
#[test]
fn an_undeclared_extension_is_not_served() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 7, "{listed}");
    assert!(
        tools
            .iter()
            .all(|t| !t["name"].as_str().unwrap().starts_with("tower__")),
        "{listed}"
    );

    let refused = server.request(
        3,
        "tools/call",
        json!({ "name": "tower__brief", "arguments": { "flight": 98 } }),
    );
    assert!(refused.get("error").is_some(), "{refused}");
    assert!(refused.get("result").is_none());

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// `isError` is the envelope's kind and not the exit code. A produced tool
/// doing what `ff pull` does — a `data` envelope at 3 for a held outcome
/// with a report — comes back a successful call carrying the data, with
/// the code in `_meta.exit`; an error envelope is an error and carries its
/// code the same way; and a help page carries its 0.
///
/// Unix only, for the reason `tests/extension.rs` is: the extension has to
/// be a real binary, and a shell script is the smallest one to write. PATH
/// is pinned to the test's own directory rather than prepended, so a
/// machine with a real `ff-tower` installed cannot answer in its place.
#[cfg(unix)]
#[test]
fn a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief", "hold"], true, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let held = call(&mut server, 2, "tower__hold", json!({}));
    assert_ne!(
        held["isError"], true,
        "a held outcome is not an error: {held}"
    );
    assert_eq!(held["structuredContent"]["cmd"], "tower hold");
    assert_eq!(held["structuredContent"]["data"]["held"], json!(["topic"]));
    assert_eq!(held["_meta"]["exit"], 3, "{held}");

    // An error envelope is an error, and its code rides the same slot.
    let missing = call(&mut server, 3, "explain", json!({ "id": "no/such-id" }));
    assert_eq!(missing["isError"], true, "{missing}");
    assert_eq!(missing["_meta"]["exit"], 2, "{missing}");

    // And a help page, text with no envelope, still says how it exited.
    let help = call(&mut server, 4, "help", json!({ "verb": ["status"] }));
    assert_ne!(help["isError"], true, "{help}");
    assert_eq!(help["_meta"]["exit"], 0, "{help}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// The client's session reaches a declared extension the way fufu's own
/// does: the tag travels as `--session` to the child `ff`, which sets
/// `FF_SESSION` before it execs `ff-<name>`.
///
/// Unix only, for the reason `a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta` is.
#[cfg(unix)]
#[test]
fn the_clients_session_reaches_a_declared_extension() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief"], true, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(
        home,
        &fx.path(),
        &[],
        &[
            ("PATH", path.as_str()),
            (
                "CLAUDE_CODE_SESSION_ID",
                "95b36d9d-efdc-4564-9b06-91842f51ef6b",
            ),
        ],
    );
    handshake(&mut server);

    let brief = call(&mut server, 2, "tower__brief", json!({ "flight": 98 }));
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(brief["structuredContent"]["cmd"], "tower brief");
    assert_eq!(
        brief["structuredContent"]["data"]["session"], "95b36d9d-efdc-4564-9b06-91842f51ef6b",
        "{brief}"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// `help` with a declared extension's name is `ff help <name>`, which
/// delegates to the extension, and comes back the way a builtin's help
/// does: text, and no structured content, because `--json` never rides a
/// `help` call and the extension's page is not one envelope.
///
/// Unix only, for the reason `a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta` is.
#[cfg(unix)]
#[test]
fn a_declared_extensions_help_is_text_with_no_structured_content() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(
        &script,
        "#!/bin/sh\nif [ \"$1\" = \"help\" ]; then\n  echo \"tower's own help page\"\n  exit 0\nfi\n",
    )
    .expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare(home.path(), "tower", &["next"], true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let help = call(&mut server, 2, "help", json!({ "verb": ["tower"] }));
    assert_ne!(help["isError"], true, "{help}");
    assert!(help.get("structuredContent").is_none(), "{help}");
    assert_eq!(
        help["content"][0]["text"], "tower's own help page",
        "{help}"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A declared extension that promised tools is asked once, when the server
/// starts, and each descriptor it answered with is listed after fufu's
/// seven under `<extension>__<tool>`. A call on one routes through the
/// same child, so the envelope comes back the way fufu's own does — and
/// the object the client sent arrives as a command line, with `cwd` lifted
/// out of it the way it is for every tool.
///
/// Unix only, for the reason `a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta` is.
#[cfg(unix)]
#[test]
fn a_promised_tool_is_listed_beside_the_seven_and_routes_to_the_verb() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief", "hold"], true, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(
        tools.len(),
        9,
        "fufu's seven, and the two tower produced: {listed}"
    );
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(&names[..7], &SEVEN, "the seven first");
    assert_eq!(&names[7..], &["tower__brief", "tower__hold"]);
    assert_eq!(tools[7]["description"], "One flight, whole.");
    assert_eq!(tools[7]["inputSchema"]["type"], "object");
    assert_eq!(
        tools[7]["inputSchema"]["properties"]["cwd"]["type"], "string",
        "a produced tool takes cwd too: {}",
        tools[7]
    );
    assert_eq!(tools[7]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[7]["annotations"]["destructiveHint"], false);

    // The arguments object becomes the command line the extension sees:
    // the positional as a bare word, the rest as options, `--json` last.
    let brief = call(
        &mut server,
        3,
        "tower__brief",
        json!({ "flight": 98, "board": "ff tower" }),
    );
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(brief["structuredContent"]["cmd"], "tower brief");
    assert_eq!(
        brief["structuredContent"]["data"]["argv"],
        "brief 98 --board ff tower --json"
    );

    // A cwd on a produced call runs it there, and is never a word.
    let there = Fixture::new();
    there.write("elsewhere.txt", "x\n");
    there.commit("elsewhere");
    let there_path = there.path();
    let brief = call(
        &mut server,
        4,
        "tower__brief",
        json!({ "flight": 98, "cwd": there_path.to_str().unwrap() }),
    );
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(
        brief["structuredContent"]["data"]["argv"],
        "brief 98 --json"
    );
    assert_eq!(
        brief["structuredContent"]["data"]["pwd"],
        std::fs::canonicalize(&there_path)
            .unwrap()
            .to_str()
            .unwrap(),
        "{brief}"
    );

    // And a name nothing here answers to is a protocol error, since no
    // child ever ran and there is no envelope to hand over.
    let unknown = server.request(5, "tools/call", json!({ "name": "bay__warm" }));
    assert!(unknown["error"]["message"].as_str().is_some(), "{unknown}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A produced tool carries the hints it stated about itself, so what the
/// manifest says under `undoable` decides nothing here: `false` is what
/// `ff extension add` reported, and the tool is listed and runs.
///
/// Unix only, for the reason `a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta` is.
#[cfg(unix)]
#[test]
fn a_promised_tool_is_served_whatever_the_manifest_says_about_undoable() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief", "hold"], false, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 9, "fufu's seven, and tower's two: {listed}");
    assert_eq!(tools[7]["name"], "tower__brief");
    assert_eq!(tools[7]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[7]["annotations"]["destructiveHint"], false);

    // And it runs: the child is the same ordinary invocation.
    let brief = call(&mut server, 3, "tower__brief", json!({ "flight": 98 }));
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(
        brief["structuredContent"]["data"]["argv"],
        "brief 98 --json"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A handshake that hangs costs the server nothing: the ask is time-boxed,
/// the binary is killed when the box expires, and what is lost is the tools
/// it promised. Nothing is said about it, on the trigger doctrine.
///
/// Unix only, for the reason `a_held_outcome_is_data_at_exit_3_and_the_code_rides_meta` is.
#[cfg(unix)]
#[test]
fn an_extension_that_hangs_on_the_handshake_costs_the_server_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    // `sleep` by absolute path: PATH is pinned to this directory, so a
    // bare one would not be found and the script would exit at once.
    std::fs::write(&script, "#!/bin/sh\n/bin/sleep 30\n").expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief"], true, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 7, "the seven, and no complaint: {listed}");

    let (code, stderr) = server.close();
    assert_eq!(code, 0);
    assert_eq!(stderr, "", "nothing is said about it");
}

/// An extension answering the tools handshake with two descriptors, and
/// echoing its own argv, its session, and its directory back so a test can
/// read the command line fufu built and where it ran it. `hold` exits 3
/// with a data envelope, the way a held `ff pull` does.
#[cfg(unix)]
const TOWER: &str = r#"#!/bin/sh
if [ "$1" = "--ff-tools" ]; then
  echo '{"ff":1,"cmd":"tower --ff-tools","data":[{"name":"brief","description":"One flight, whole.","inputSchema":{"type":"object","positional":["flight"],"properties":{"flight":{"type":"integer"},"board":{"type":"string"}}},"annotations":{"readOnlyHint":true,"destructiveHint":false}},{"name":"hold","description":"A held outcome.","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":false,"destructiveHint":false}}]}'
  exit 0
fi
if [ "$1" = "hold" ]; then
  echo '{"ff":1,"cmd":"tower hold","data":{"held":["topic"]}}'
  exit 3
fi
printf '{"ff":1,"cmd":"tower %s","data":{"argv":"%s","session":"%s","pwd":"%s"}}\n' "$1" "$*" "$FF_SESSION" "$(pwd -P)"
"#;
