//! `ff mcp`: the one tool over stdio, driven by hand in both protocol eras.
//!
//! Every test spawns the real binary as a server, writes JSON-RPC lines to
//! its stdin, and reads lines from its stdout. No client library, on
//! purpose: what a client sends is the contract, and a library would hide
//! which era's shape was being spoken.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use serde_json::{Value, json};

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// Responses that arrived while waiting for another id. Calls run
    /// concurrently, so answers may come back in any order.
    parked: HashMap<u64, Value>,
}

fn start(dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Server {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
        .arg("mcp")
        .args(extra)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("FF_SESSION")
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
    }
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
    fn close(mut self) -> (i32, String) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait");
        let mut stderr = String::new();
        if let Some(mut err) = self.child.stderr.take() {
            std::io::Read::read_to_string(&mut err, &mut stderr).expect("read stderr");
        }
        (status.code().unwrap_or(-1), stderr)
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

fn call(server: &mut Server, id: u64, args: &[&str]) -> Value {
    server.request(
        id,
        "tools/call",
        json!({ "name": "ff", "arguments": { "args": args } }),
    )["result"]
        .clone()
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

// ---- the legacy era --------------------------------------------------------

#[test]
fn the_legacy_handshake_lists_one_tool_and_relays_the_envelope() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);

    let init = handshake(&mut server);
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["serverInfo"]["name"], "fufu");
    assert!(
        init["result"]["instructions"]
            .as_str()
            .is_some_and(|s| s.starts_with("fufu (`ff`) is capturing")),
        "the briefing is the instructions: {init}"
    );

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 1, "one tool: {listed}");
    assert_eq!(tools[0]["name"], "ff");
    let description = tools[0]["description"].as_str().expect("a description");
    assert!(description.contains("commit"), "the verb list is in it");
    assert!(
        description.contains("## Recovery"),
        "the recovery table is in it"
    );
    assert!(
        description.contains("## Landmines"),
        "the landmines are in it"
    );
    assert_eq!(tools[0]["inputSchema"]["required"], json!(["args"]));

    // A reader: the envelope comes back whole, as text and as structure.
    let status = call(&mut server, 3, &["status"]);
    assert_ne!(status["isError"], true, "{status}");
    assert_eq!(status["structuredContent"]["ff"], 1);
    assert_eq!(status["structuredContent"]["cmd"], "status");
    assert_eq!(status["structuredContent"]["data"]["head"]["name"], "main");
    let text = status["content"][0]["text"].as_str().expect("text content");
    let parsed: Value = serde_json::from_str(text).expect("the text is the envelope");
    assert_eq!(parsed, status["structuredContent"]);

    // A fufu failure is a successful call carrying is_error and the id.
    let missing = call(&mut server, 4, &["show", "doesnotexist"]);
    assert_eq!(missing["isError"], true, "{missing}");
    assert_eq!(
        missing["structuredContent"]["error"]["id"],
        "usage/revset-unknown-revision"
    );

    // An excluded verb is refused by id, without running anything.
    let git = call(&mut server, 5, &["git", "status"]);
    assert_eq!(git["isError"], true);
    assert_eq!(
        git["structuredContent"]["error"]["id"],
        "usage/mcp-verb-unavailable"
    );
    assert_eq!(git["structuredContent"]["cmd"], "git");

    // Help is text, and only text.
    let help = call(&mut server, 6, &["help", "log"]);
    assert_ne!(help["isError"], true, "{help}");
    assert!(help.get("structuredContent").is_none(), "{help}");
    assert!(
        help["content"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("Usage: ff log")),
        "{help}"
    );

    // Nothing at all is refused the same way, and names the map.
    let empty = call(&mut server, 7, &[]);
    assert_eq!(empty["isError"], true);
    assert_eq!(
        empty["structuredContent"]["error"]["id"],
        "usage/mcp-verb-unavailable"
    );

    let (code, stderr) = server.close();
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
    let status = server.request(
        2,
        "tools/call",
        json!({
            "name": "ff",
            "arguments": { "args": ["log", "--commits", "-n", "1"], "cwd": there_path.to_str().unwrap() }
        }),
    )["result"]
        .clone();
    assert_ne!(status["isError"], true, "{status}");
    assert_eq!(
        status["structuredContent"]["data"]["commits"][0]["subject"],
        "elsewhere"
    );
    let (code, _) = server.close();
    assert_eq!(code, 0);
}

#[test]
fn malformed_input_is_a_protocol_error_not_a_tool_result() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    handshake(&mut server);

    let no_args = server.request(2, "tools/call", json!({ "name": "ff", "arguments": {} }));
    assert!(no_args.get("error").is_some(), "{no_args}");
    assert!(no_args.get("result").is_none());

    let not_strings = server.request(
        3,
        "tools/call",
        json!({ "name": "ff", "arguments": { "args": ["status", 1] } }),
    );
    assert!(not_strings.get("error").is_some(), "{not_strings}");

    let wrong_tool = server.request(
        4,
        "tools/call",
        json!({ "name": "git", "arguments": { "args": ["status"] } }),
    );
    assert!(wrong_tool.get("error").is_some(), "{wrong_tool}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// `--session` on the server tags every child's operations, which is how
/// an agent's work through the tool stays separable from a person's.
#[test]
fn the_servers_session_rides_every_child() {
    let fx = repo();
    fx.write("b.txt", "b\n");
    let mut server = start(&fx.path(), &["--session", "flight-3"], &[]);
    handshake(&mut server);

    let commit = call(&mut server, 2, &["commit", "-m", "through the tool"]);
    assert_ne!(commit["isError"], true, "{commit}");
    assert_eq!(commit["structuredContent"]["cmd"], "commit");

    let ops = call(&mut server, 3, &["op", "log", "kind(op)"]);
    let op = &ops["structuredContent"]["data"]["ops"][0];
    assert_eq!(op["verb"], "commit", "{ops}");
    assert_eq!(op["session"], "flight-3", "{ops}");

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
    let commit = call(&mut server, 2, &["commit", "-m", "env"]);
    assert_ne!(commit["isError"], true, "{commit}");
    let ops = call(&mut server, 3, &["op", "log", "kind(op)"]);
    assert_eq!(
        ops["structuredContent"]["data"]["ops"][0]["session"],
        "from-env"
    );
    server.close();

    fx.write("c.txt", "c\n");
    let mut server = start(
        &fx.path(),
        &["--session", "from-flag"],
        &[("FF_SESSION", "from-env")],
    );
    handshake(&mut server);
    let commit = call(&mut server, 2, &["commit", "-m", "flag"]);
    assert_ne!(commit["isError"], true, "{commit}");
    let ops = call(&mut server, 3, &["op", "log", "kind(op)"]);
    assert_eq!(
        ops["structuredContent"]["data"]["ops"][0]["session"],
        "from-flag"
    );
    server.close();
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
        json!({ "_meta": meta, "name": "ff", "arguments": { "args": ["status"] } }),
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
