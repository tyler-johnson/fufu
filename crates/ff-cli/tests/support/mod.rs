//! The live suites' harness: a real agent client in a scratch HOME, wired
//! with `ff hook`, then asked what it loaded or run headless against
//! `ff_testsupport::mock_model`.
//!
//! A directory module rather than a file so it is not itself a test
//! crate; each suite that says `mod support;` compiles its own copy, and
//! the allow below is for the helpers that copy does not reach.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ff_testsupport::fixtures::null_device;
use ff_testsupport::{Fixture, scrub};

/// The head of the briefing `ff trigger` answers a context event with —
/// the prose is `integ/briefing.rs::NOTICE`, and this is the substring the
/// suites look for in a client's request body.
pub const NOTICE_HEAD: &str = "fufu (`ff`) takes snapshots";

/// The head of `ff op log --json`'s envelope: what the mock model waits
/// for in a request body before ending the turn, and what
/// [`op_log_from`] finds in a tool result.
pub const OP_LOG_HEAD: &str = r#"{"ff":1,"cmd":"op log""#;
pub const OP_LOG_MARKER: &str = r#""cmd":"op log""#;

/// The two shell lines the mock model asks the client to run, in order.
/// The first moves the tree, so the second's pre-tool hook has something
/// to capture that no earlier event took; the second reads the log back,
/// and its top entry is that capture — labeled with the second command,
/// which is the proof the tool event fired and captured before the tool
/// ran. The second names this build's `ff` by its absolute path: Codex
/// runs its shell tool through a login shell, whose profile resets
/// `PATH`.
pub const TOUCH: &str = "touch b.txt";

pub fn op_log() -> String {
    format!("{} op log --json -n 1", ff_bin())
}

pub fn commands() -> [String; 2] {
    [TOUCH.to_string(), op_log()]
}

/// A capture's label for a shell command: the subject line the runtime
/// cuts it to, `provenance::truncate`'s rule.
pub fn label(command: &str) -> String {
    const MAX: usize = 64;
    if command.chars().count() <= MAX {
        return command.to_string();
    }
    let mut out: String = command.chars().take(MAX - 1).collect();
    out.push('…');
    out
}

/// The client binary on `PATH`, or the reason the suite is not running.
/// Absent and `FF_LIVE` unset, the suite skips with a line on stderr;
/// absent and `FF_LIVE` set, it panics — a CI job whose install failed
/// must not pass by skipping.
pub fn live_client(name: &str) -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            [
                name.to_string(),
                format!("{name}.exe"),
                format!("{name}.cmd"),
            ]
            .into_iter()
            .map(|file| dir.join(file))
            .find(|candidate| candidate.is_file())
        })
    });
    if found.is_none() {
        assert!(
            std::env::var_os("FF_LIVE").is_none(),
            "FF_LIVE is set and {name} is not on PATH"
        );
        eprintln!("skipping: {name} is not on PATH");
    }
    found
}

/// A repository with one commit and a dirty tree, so the first hook the
/// client fires has something to capture.
pub fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    fx
}

/// The suite's `HOME`: `home/` beside the fixture's `repo/`, with the
/// client's config directory already there so `ff hook` sees the client
/// as present, and the XDG roots beside it.
pub fn scratch_home(fx: &Fixture, client_dir: &str) -> PathBuf {
    let home = fx.root().join("home");
    for dir in [client_dir, "xdg", "cache"] {
        std::fs::create_dir_all(home.join(dir)).expect("mkdir under the scratch home");
    }
    home
}

/// Address a spawn — the client's or `ff`'s — at the scratch home: `HOME`
/// and its Windows twin, the XDG roots and the update cache under it, the
/// developer's git config and client overrides out of reach, the agent
/// variables scrubbed, this build's `ff` first on `PATH`, the working
/// directory set — as `PWD` too, which OpenCode reads ahead of the
/// process's own cwd — and stdin closed so nothing can wait on a
/// terminal.
pub fn client_env(command: &mut Command, home: &Path, cwd: &Path) {
    command
        .current_dir(cwd)
        .env("PWD", cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("APPDATA", home.join("xdg"))
        .env("LOCALAPPDATA", home.join("cache"))
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_CODE_EXECPATH")
        .env_remove("CODEX_HOME")
        .env_remove("COPILOT_HOME")
        .env_remove("OPENCODE_CONFIG_DIR")
        .env_remove("FF_CODEX")
        .env_remove("FF_OPENCODE")
        .env_remove("FF_COPILOT")
        .env_remove("FF_CURSOR")
        .stdin(Stdio::null());
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with("ANTHROPIC_") || name.starts_with("OPENAI_") {
            command.env_remove(&*name);
        }
    }
    scrub(command);
    let bin = Path::new(env!("CARGO_BIN_EXE_ff"))
        .parent()
        .expect("the binary has a directory")
        .to_path_buf();
    let mut paths = vec![bin];
    if let Some(inherited) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    command.env(
        "PATH",
        std::env::join_paths(paths).expect("a joinable PATH"),
    );
}

/// `ff hook <slug>` into the scratch home from the fixture repository, the
/// real client on `PATH` — for Codex, the real `codex plugin add` runs.
/// Both streams are in the panic when it fails.
pub fn hook(home: &Path, repo: &Path, slug: &str) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ff"));
    client_env(&mut command, home, repo);
    let out = command
        .args(["hook", slug])
        .output()
        .expect("spawn ff hook");
    assert!(
        out.status.success(),
        "`ff hook {slug}` exited {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// `ff unhook <slug>` the same way.
pub fn unhook(home: &Path, repo: &Path, slug: &str) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ff"));
    client_env(&mut command, home, repo);
    let out = command
        .args(["unhook", slug])
        .output()
        .expect("spawn ff unhook");
    assert!(
        out.status.success(),
        "`ff unhook {slug}` exited {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// This build's `ff`, as an absolute path.
pub fn ff_bin() -> &'static str {
    env!("CARGO_BIN_EXE_ff")
}

/// Run a spawn to completion within `timeout`, stdout captured and stderr
/// inherited so a client's warnings are in the log under `--nocapture`.
/// Killed and failed on the deadline, with what it said so far in the
/// panic: a client that hangs on a prompt is a finding, not a stall. The
/// reader thread is never joined on that path — a killed client's own
/// children can hold the pipe open past it, and a join would wait on
/// them.
pub fn run_within(command: &mut Command, timeout: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = command.spawn().expect("spawn the client");
    let mut stdout = child.stdout.take().expect("piped stdout");
    // Drained on its own thread into a shared buffer: a client that fills
    // the pipe would block behind a waiting parent.
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    let reader = std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink
                    .lock()
                    .expect("the buffer")
                    .extend_from_slice(&chunk[..n]),
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll the client") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let so_far = buffer.lock().expect("the buffer").clone();
            panic!(
                "the client ran past {}s\nstdout so far: {}",
                timeout.as_secs(),
                String::from_utf8_lossy(&so_far)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = reader.join();
    let stdout = std::mem::take(&mut *buffer.lock().expect("the buffer"));
    Output {
        status,
        stdout,
        stderr: Vec::new(),
    }
}

/// The `op log --json` envelope out of a tool result: the clients wrap
/// tool output differently (Codex leads with a chunk id and `Output:`),
/// so the envelope is found by its head and parsed from there.
pub fn op_log_from(text: &str) -> serde_json::Value {
    let start = text
        .find(OP_LOG_HEAD)
        .unwrap_or_else(|| panic!("no op log envelope in the tool output: {text}"));
    let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter();
    stream
        .next()
        .expect("an envelope")
        .unwrap_or_else(|err| panic!("the envelope does not parse: {err}\n{text}"))
}

/// The proof one tool result carries: the top operation the client's own
/// tool read back is the capture the client's pre-tool hook took under
/// `session` before that very tool ran — the tree the first command
/// moved, labeled with the second command under the client's own name
/// for its shell tool.
pub fn captured_by(envelope: &serde_json::Value, client: &str, session: &str, tool: &str) {
    assert_eq!(envelope["cmd"], "op log", "{envelope}");
    let op = &envelope["data"]["ops"][0];
    assert_eq!(op["kind"], "capture", "the top op is a capture: {envelope}");
    let summary = format!(
        "{client}[{}]: {tool}({})",
        session.chars().take(8).collect::<String>(),
        label(&op_log())
    );
    assert_eq!(op["summary"], summary, "{envelope}");
    assert_eq!(op["session"], session, "the session trailer: {envelope}");
}

/// The weaker proof, for a client whose continuation after a tool result
/// fires its own prompt event ahead of the next tool call and takes the
/// capture first: the top operation is a capture the client's hooks took
/// under `session`, whichever event took it.
pub fn captured_under(envelope: &serde_json::Value, client: &str, session: &str) {
    assert_eq!(envelope["cmd"], "op log", "{envelope}");
    let op = &envelope["data"]["ops"][0];
    assert_eq!(op["kind"], "capture", "the top op is a capture: {envelope}");
    let prefix = format!(
        "{client}[{}]: ",
        session.chars().take(8).collect::<String>()
    );
    assert!(
        op["summary"]
            .as_str()
            .is_some_and(|summary| summary.starts_with(&prefix)),
        "the capture names the client and its session ({prefix:?}): {envelope}"
    );
    assert_eq!(op["session"], session, "the session trailer: {envelope}");
}

/// A line reader on a child's stdout, with a deadline: lines arrive on a
/// channel from a thread, so a client that goes quiet ends the wait
/// rather than the test.
pub struct Lines {
    receiver: mpsc::Receiver<String>,
}

impl Lines {
    pub fn of(stdout: impl Read + Send + 'static) -> Lines {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Lines { receiver }
    }

    /// The next line before `deadline`, or `None` at EOF or on the
    /// deadline.
    pub fn next_before(&self, deadline: Instant) -> Option<String> {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        self.receiver.recv_timeout(deadline - now).ok()
    }
}
