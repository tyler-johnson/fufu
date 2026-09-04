//! `ff mcp`: the one tool over stdio, driven by hand in both protocol eras.
//!
//! Every test spawns the real binary as a server, writes JSON-RPC lines to
//! its stdin, and reads lines from its stdout. No client library, on
//! purpose: what a client sends is the contract, and a library would hide
//! which era's shape was being spoken.
//!
//! The test process is the server's parent, so the presence marker a
//! serving instance holds is the one under this process's pid, named for
//! the server it registers as, under a cache root pinned per test so the
//! real user cache is never touched. The
//! config root is pinned beside it, since the extension registry lives
//! there and decides what the tool serves and what its card names.

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
    cmd.current_dir(dir)
        .arg("mcp")
        .args(extra)
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", cache_under(home.path()))
        .env("LOCALAPPDATA", cache_under(home.path()))
        .env("XDG_CONFIG_HOME", config_under(home.path()))
        .env("APPDATA", config_under(home.path()))
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
        home,
    }
}

/// Where the binary resolves its cache root under `home`, given the
/// variables `start` pins: macOS reads only HOME, and the other two
/// platforms read the variable pinned here.
fn cache_under(home: &Path) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else {
        home.join(".cache")
    }
}

/// Where the binary resolves its config root under `home`, and so where it
/// reads the extension registry.
///
/// Pinned for the reason the cache root is, and for one more: the registry
/// decides which extensions the tool serves and names on its card, and a
/// developer running this suite has a registry of their own. macOS reads
/// only HOME; the other two platforms read a variable `start` pins.
fn config_under(home: &Path) -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".config")
    }
}

/// Declare one extension on the machine `home` stands for, the way
/// `ff extension add` records one.
fn declare(home: &Path, name: &str, verbs: &[&str], undoable: bool) {
    declare_promising(home, name, verbs, undoable, false);
}

/// [`declare`], with the manifest promising tools or not.
fn declare_promising(home: &Path, name: &str, verbs: &[&str], undoable: bool, tools: bool) {
    let dir = config_under(home).join("fufu");
    std::fs::create_dir_all(&dir).expect("the config root");
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
    std::fs::write(dir.join("extensions.json"), body.to_string()).expect("write the registry");
}

/// `<cache>/fufu/mcp/` under a scratch HOME, which holds a directory per
/// client and a marker per server name inside it.
fn marker_dir(home: &Path) -> std::path::PathBuf {
    cache_under(home).join("fufu").join("mcp")
}

/// One client's directory of markers.
fn client_dir(home: &Path, client: u32) -> std::path::PathBuf {
    marker_dir(home).join(client.to_string())
}

/// fufu's own marker, for a server this process spawned.
fn marker(home: &Path) -> std::path::PathBuf {
    client_dir(home, std::process::id()).join("fufu")
}

/// Nothing under the marker directory, or no directory at all.
fn assert_no_marker(home: &Path) {
    if let Ok(entries) = std::fs::read_dir(marker_dir(home)) {
        let names: Vec<_> = entries.map(|e| e.unwrap().file_name()).collect();
        assert!(names.is_empty(), "no marker was written: {names:?}");
    }
}

impl Server {
    /// The marker is there and a live server holds it: a shared lock is
    /// refused. Deterministic once any post-handshake response has been
    /// read, because the hold happens before the request loop starts.
    fn assert_serving(&self) {
        let path = marker(self.home.path());
        let file = std::fs::File::open(&path)
            .unwrap_or_else(|err| panic!("the marker {} exists: {err}", path.display()));
        match file.try_lock_shared() {
            Err(std::fs::TryLockError::WouldBlock) => {}
            other => panic!("the server holds its marker exclusively: {other:?}"),
        }
    }

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

    /// The same, handing back the scratch HOME so a marker assertion after
    /// the exit reads a directory that is still there.
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
fn a_starting_server_sweeps_the_markers_nobody_holds() {
    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    // Left by a client that is gone: nothing holds it.
    let gone = client_dir(home.path(), 4242);
    std::fs::create_dir_all(&gone).unwrap();
    let stale = gone.join("fufu");
    std::fs::write(&stale, "{\"server\":1}\n").unwrap();
    // Held the way a live server holds its own.
    let live = client_dir(home.path(), 4243).join("fufu");
    std::fs::create_dir_all(live.parent().unwrap()).unwrap();
    let lock = std::fs::File::create(&live).unwrap();
    lock.try_lock().expect("an exclusive lock");

    let mut server = start_in(home, &fx.path(), &[], &[]);
    handshake(&mut server);
    server.request(2, "tools/list", Value::Null);
    server.assert_serving();
    assert!(!stale.exists(), "the stale marker was swept at start");
    assert!(
        !gone.exists(),
        "and the directory of a client with nothing left went with it"
    );
    assert!(
        live.is_file(),
        "a marker another server holds is left alone"
    );
    drop(lock);
    server.close();
}

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
    // Serving, and provably so: the marker for this process — the
    // server's parent — is held for as long as it serves.
    server.assert_serving();
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 1, "one tool: {listed}");
    assert_eq!(tools[0]["name"], "ff");
    let description = tools[0]["description"].as_str().expect("a description");
    assert!(description.contains(", commit"), "the verb list is in it");
    assert!(
        description.contains("\nRecovery: "),
        "the recovery digest is in it"
    );
    assert!(
        description.contains("\nLandmines: "),
        "the landmines are in it"
    );
    assert!(
        description.chars().count() < 2_048,
        "the card fits what a client shows the model: {}",
        description.chars().count()
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

    // And `extension` with it, which is the one excluded for what it
    // writes rather than for what it prints: the registry is the allowlist
    // for everything fufu says about an extension, so an agent putting a
    // name on it would be deciding for itself what fufu vouches for.
    let declare = call(&mut server, 6, &["extension", "add", "tower"]);
    assert_eq!(declare["isError"], true);
    assert_eq!(
        declare["structuredContent"]["error"]["id"],
        "usage/mcp-verb-unavailable"
    );
    assert_eq!(declare["structuredContent"]["cmd"], "extension");

    // Help is text, and only text.
    let help = call(&mut server, 7, &["help", "log"]);
    assert_ne!(help["isError"], true, "{help}");
    assert!(help.get("structuredContent").is_none(), "{help}");
    assert!(
        help["content"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("Usage: ff log")),
        "{help}"
    );

    // Nothing at all is refused the same way, and names the map.
    let empty = call(&mut server, 8, &[]);
    assert_eq!(empty["isError"], true);
    assert_eq!(
        empty["structuredContent"]["error"]["id"],
        "usage/mcp-verb-unavailable"
    );

    let (code, stderr, home) = server.shutdown();
    assert_eq!(code, 0, "closing stdin ends the server cleanly");
    assert_eq!(stderr, "", "nothing on stderr without FF_DEBUG");
    assert!(
        !marker(home.path()).exists(),
        "the marker went with the server"
    );
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
    // The new era marks too: `server/discover` is where serving begins.
    server.assert_serving();

    let (code, stderr) = server.close();
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
}

/// A client that opens the pipe and closes it again, which is how a client
/// probes whether a server starts, gets a clean exit and no complaint —
/// and no marker, because a probe is not a server that is up.
#[test]
fn closing_stdin_before_speaking_exits_zero() {
    let fx = repo();
    let server = start(&fx.path(), &[], &[]);
    let (code, stderr, home) = server.shutdown();
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
    assert_no_marker(home.path());
}

#[test]
fn a_policy_key_is_not_writable_through_the_tool() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    handshake(&mut server);

    // The write the tool exists to police, refused by id.
    for words in [
        vec!["config", "toolPolicy", "observe"],
        vec!["config", "gitPolicy", "observe"],
        vec!["config", "--global", "toolPolicy", "observe"],
        vec!["config", "--unset", "gitPolicy"],
        // The spelling does not matter: clap resolves it before the check.
        vec!["config", "fufu.toolpolicy", "coach"],
    ] {
        let refused = call(&mut server, 2, &words);
        assert_eq!(refused["isError"], true, "{words:?}: {refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["id"], "usage/mcp-policy-write",
            "{words:?}: {refused}"
        );
    }

    // Nothing was written: the tier still reads as its default.
    let read = call(&mut server, 3, &["config", "toolPolicy"]);
    assert_ne!(read["isError"], true, "{read}");
    assert_eq!(read["structuredContent"]["data"]["value"], "strict");
    assert_eq!(read["structuredContent"]["data"]["default"], true);

    // Listing is a read too, and still lists every setting.
    let listed = call(&mut server, 4, &["config"]);
    assert_ne!(listed["isError"], true, "{listed}");
    assert_eq!(
        listed["structuredContent"]["data"]["settings"]
            .as_array()
            .expect("a settings array")
            .len(),
        12
    );

    // Every other key writes through the tool as before.
    let written = call(&mut server, 5, &["config", "keep", "45d"]);
    assert_ne!(written["isError"], true, "{written}");
    assert_eq!(written["structuredContent"]["data"]["value"], "45d");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

// ---- extensions ------------------------------------------------------------

/// The tool serves the extensions a person declared. An `ff <name>` on no
/// registry is refused before anything runs, and the exit names the
/// declaration — `fufu.toolPolicy` lets the same call through a shell, so
/// between the two there is always one place it runs.
#[test]
fn an_undeclared_extension_is_refused_by_id() {
    let fx = repo();
    let mut server = start(&fx.path(), &[], &[]);
    handshake(&mut server);

    let refused = call(&mut server, 2, &["tower", "next"]);
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["id"],
        "usage/mcp-extension-undeclared"
    );
    assert_eq!(refused["structuredContent"]["cmd"], "tower");
    let exits = refused["structuredContent"]["error"]["exits"]
        .as_array()
        .expect("exits");
    assert!(
        exits.iter().any(|exit| exit == "ff extension add tower"),
        "{refused}"
    );

    // Nothing on the card either, since nothing is declared.
    let listed = server.request(3, "tools/list", Value::Null);
    let description = listed["result"]["tools"][0]["description"]
        .as_str()
        .expect("a description");
    assert!(!description.contains("Extensions: "), "{description}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A declared extension is served the way a verb is: the child dispatches
/// to `ff-<name>` and the envelope it printed reaches the agent as
/// structured content. The card names it with the verbs its manifest lists.
///
/// Unix only, for the reason `tests/extension.rs` is: the extension has to
/// be a real binary, and a shell script is the smallest one to write. PATH
/// is pinned to the test's own directory rather than prepended, so a
/// machine with a real `ff-tower` installed cannot answer in its place.
#[cfg(unix)]
#[test]
fn a_declared_extension_is_served_and_named_on_the_card() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(
        &script,
        "#!/bin/sh\necho '{\"ff\":1,\"cmd\":\"tower next\",\"data\":{\"flight\":68}}'\n",
    )
    .expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare(home.path(), "tower", &["next", "file", "done"], true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let description = listed["result"]["tools"][0]["description"]
        .as_str()
        .expect("a description");
    assert!(
        description.contains("\nExtensions: tower (next, file, done)\n"),
        "{description}"
    );
    assert!(
        description.chars().count() < 2_048,
        "the card still fits what a client shows the model: {}",
        description.chars().count()
    );

    let next = call(&mut server, 3, &["tower", "next"]);
    assert_ne!(next["isError"], true, "{next}");
    assert_eq!(next["structuredContent"]["ff"], 1);
    assert_eq!(next["structuredContent"]["cmd"], "tower next");
    assert_eq!(next["structuredContent"]["data"]["flight"], 68);

    // And declaring one name says nothing about another.
    let refused = call(&mut server, 4, &["bay", "warm"]);
    assert_eq!(
        refused["structuredContent"]["error"]["id"],
        "usage/mcp-extension-undeclared"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A help call for a declared extension goes through the same relay a
/// verb does — `refuse_in` already lets `help` and `explain` by as builtin
/// words — and comes back the way a builtin's help does: text, and no
/// structured content, because `wants_json` never rides a `help` call and
/// the extension's page is not one envelope.
///
/// Unix only, for the reason `a_declared_extension_is_served_and_named_on_the_card` is.
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

    let help = call(&mut server, 2, &["help", "tower"]);
    assert_ne!(help["isError"], true, "{help}");
    assert!(help.get("structuredContent").is_none(), "{help}");
    assert_eq!(
        help["content"][0]["text"], "tower's own help page",
        "{help}"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// The one tool's annotations say that nothing it relays is destructive,
/// which is honest only of an extension whose writes `ff undo` takes back.
/// One declaring otherwise, and promising no tools of its own, is refused
/// on the args array and has the shell — and stays on the card, the way the
/// shell-only verbs stay on it, because an agent told where to run
/// something has to know the word.
#[test]
fn an_extension_that_is_not_undoable_is_refused_on_the_args_array() {
    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    declare(home.path(), "tower", &["next"], false);
    let mut server = start_in(home, &fx.path(), &[], &[]);
    handshake(&mut server);

    let refused = call(&mut server, 2, &["tower", "next"]);
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["id"],
        "usage/mcp-extension-not-undoable"
    );

    let listed = server.request(3, "tools/list", Value::Null);
    let description = listed["result"]["tools"][0]["description"]
        .as_str()
        .expect("a description");
    assert!(
        description.contains("Extensions: tower (next)"),
        "{description}"
    );

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A declared extension that promised tools is asked once, when the server
/// starts, and each descriptor it answered with is listed beside fufu's own
/// tool under `<extension>__<tool>`. A call on one of those routes back
/// through the same child, so the envelope comes back the way a relayed
/// call's does — and the object the client sent arrives as a command line.
///
/// Unix only, for the reason `a_declared_extension_is_served_and_named_on_the_card` is.
#[cfg(unix)]
#[test]
fn a_promised_tool_is_listed_beside_the_one_tool_and_routes_to_the_verb() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief"], true, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(
        tools.len(),
        2,
        "fufu's own, and the one tower produced: {listed}"
    );
    assert_eq!(
        tools[0]["name"], "ff",
        "the pass-through route is unchanged"
    );
    assert_eq!(tools[0]["inputSchema"]["required"], json!(["args"]));
    assert_eq!(tools[1]["name"], "tower__brief");
    assert_eq!(tools[1]["description"], "One flight, whole.");
    assert_eq!(tools[1]["inputSchema"]["type"], "object");
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[1]["annotations"]["destructiveHint"], false);
    // The card says nothing new: a produced tool is already a tool in the
    // client's own list, carrying its own description.
    let description = tools[0]["description"].as_str().expect("a description");
    assert!(
        description.contains("\nExtensions: tower (brief)\n"),
        "{description}"
    );
    assert!(!description.contains("tower__brief"), "{description}");

    // The arguments object becomes the command line the extension sees:
    // the positional as a bare word, the rest as options, `--json` last.
    let brief = server.request(
        3,
        "tools/call",
        json!({
            "name": "tower__brief",
            "arguments": { "flight": 98, "board": "ff tower" }
        }),
    )["result"]
        .clone();
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(brief["structuredContent"]["cmd"], "tower brief");
    assert_eq!(
        brief["structuredContent"]["data"]["argv"],
        "brief 98 --board ff tower --json"
    );

    // And a name nothing here answers to is a protocol error, since no
    // child ever ran and there is no envelope to hand over.
    let unknown = server.request(4, "tools/call", json!({ "name": "bay__warm" }));
    assert!(unknown["error"]["message"].as_str().is_some(), "{unknown}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// The undoable gate is the args array's alone. A produced tool carries the
/// hints it stated about itself, so an extension declaring `undoable: false`
/// is listed and called on that route and refused on the other — and the
/// refusal names the route that does serve it.
///
/// Unix only, for the reason `a_declared_extension_is_served_and_named_on_the_card` is.
#[cfg(unix)]
#[test]
fn a_promised_tool_is_served_for_an_extension_the_args_array_will_not_relay() {
    use std::os::unix::fs::PermissionsExt;

    let fx = repo();
    let home = tempfile::TempDir::new().expect("a scratch HOME");
    let bin = tempfile::TempDir::new().expect("a scratch PATH");
    let script = bin.path().join("ff-tower");
    std::fs::write(&script, TOWER).expect("write the extension");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    declare_promising(home.path(), "tower", &["brief"], false, true);

    let path = bin.path().display().to_string();
    let mut server = start_in(home, &fx.path(), &[], &[("PATH", path.as_str())]);
    handshake(&mut server);

    let listed = server.request(2, "tools/list", Value::Null);
    let tools = listed["result"]["tools"].as_array().expect("a tool list");
    assert_eq!(tools.len(), 2, "fufu's own, and tower's: {listed}");
    assert_eq!(tools[1]["name"], "tower__brief");
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[1]["annotations"]["destructiveHint"], false);

    // And it runs: the child is the same ordinary invocation.
    let brief = server.request(
        3,
        "tools/call",
        json!({ "name": "tower__brief", "arguments": { "flight": 98 } }),
    )["result"]
        .clone();
    assert_ne!(brief["isError"], true, "{brief}");
    assert_eq!(
        brief["structuredContent"]["data"]["argv"],
        "brief 98 --json"
    );

    // The same verb in the args array is still refused, and the refusal
    // names both places it does run.
    let refused = call(&mut server, 4, &["tower", "brief", "98"]);
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["id"],
        "usage/mcp-extension-not-undoable"
    );
    let message = refused["structuredContent"]["error"]["message"]
        .as_str()
        .expect("a message");
    assert!(message.contains("tower__<tool>"), "{message}");
    assert!(message.contains("shell"), "{message}");

    let (code, _) = server.close();
    assert_eq!(code, 0);
}

/// A handshake that hangs costs the server nothing: the ask is time-boxed,
/// the binary is killed when the box expires, and what is lost is the tools
/// it promised. Nothing is said about it, on the trigger doctrine.
///
/// Unix only, for the reason `a_declared_extension_is_served_and_named_on_the_card` is.
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
    assert_eq!(tools.len(), 1, "one tool, and no complaint: {listed}");
    // The args-array route is untouched by a handshake that failed.
    let description = tools[0]["description"].as_str().expect("a description");
    assert!(
        description.contains("Extensions: tower (brief)"),
        "{description}"
    );

    let (code, stderr) = server.close();
    assert_eq!(code, 0);
    assert_eq!(stderr, "", "nothing is said about it");
}

/// An extension answering the tools handshake, and echoing its own argv
/// back so a test can read the command line fufu built.
#[cfg(unix)]
const TOWER: &str = r#"#!/bin/sh
if [ "$1" = "--ff-tools" ]; then
  echo '{"ff":1,"cmd":"tower --ff-tools","data":[{"name":"brief","description":"One flight, whole.","inputSchema":{"type":"object","positional":["flight"],"properties":{"flight":{"type":"integer"},"board":{"type":"string"}}},"annotations":{"readOnlyHint":true,"destructiveHint":false}}]}'
  exit 0
fi
printf '{"ff":1,"cmd":"tower %s","data":{"argv":"%s"}}\n' "$1" "$*"
"#;
