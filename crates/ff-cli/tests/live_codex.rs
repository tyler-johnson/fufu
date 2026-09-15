//! Codex against the real binary: what it loads after `ff hook codex`,
//! and what it sends once a session runs.
//!
//! `tests/installers.rs` proves what fufu writes; nothing there can see a
//! Codex that reads the plugin's skill and never its hooks. This suite is
//! the layer above: the client itself says what it loaded (`codex
//! app-server`, `hooks/list` and `plugin/list`, no session), and a
//! headless `codex exec` runs against `ff_testsupport::mock_model`, where
//! the proof is what Codex sent — the briefing in the first request, and
//! the `ff op log` output carried back, whose top entry is the capture
//! the `PreToolUse` hook took before that tool ran — and never a model's
//! judgment.
//!
//! Needs `codex` on `PATH`, and the network: `ff hook codex` runs the
//! real `codex plugin add`, which clones Codex's curated marketplace on
//! the way. Skips with a line when the binary is absent, and fails
//! instead under `FF_LIVE=1`, which is how CI runs it.

mod support;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ff_testsupport::mock_model::{MockModel, Recorded};
use serde_json::Value;
use support::{
    NOTICE_HEAD, OP_LOG_MARKER, captured_by, client_env, commands, ff_bin, hook, live_client,
    op_log_from, repo, run_within, scratch_home,
};

/// The five hook keys Codex reports for fufu's plugin, one per event in
/// `hooks/hooks.json`.
const HOOK_KEYS: [&str; 5] = [
    "fufu@fufu:hooks/hooks.json:pre_tool_use:0:0",
    "fufu@fufu:hooks/hooks.json:user_prompt_submit:0:0",
    "fufu@fufu:hooks/hooks.json:session_start:0:0",
    "fufu@fufu:hooks/hooks.json:stop:0:0",
    "fufu@fufu:hooks/hooks.json:session_end:0:0",
];

/// One `codex app-server` exchange over stdio: `initialize` and the
/// `initialized` notification, then `requests` — JSON-RPC bodies each
/// carrying an `id` — answered as a map from id to result. Replies are
/// one object per line and unsolicited notifications interleave, so the
/// reader keeps going until every id is in, or ten seconds have passed.
fn app_server(codex: &Path, home: &Path, cwd: &Path, requests: &[Value]) -> BTreeMap<u64, Value> {
    let mut command = Command::new(codex);
    client_env(&mut command, home, cwd);
    let mut child = command
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn codex app-server");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let lines = support::Lines::of(child.stdout.take().expect("piped stdout"));

    let mut wanted: Vec<u64> = Vec::new();
    let mut send = |value: Value| {
        stdin
            .write_all(format!("{value}\n").as_bytes())
            .expect("write to the app server");
    };
    send(serde_json::json!({
        "id": 1,
        "method": "initialize",
        "params": {"clientInfo": {"name": "fufu-tests", "title": "fufu tests", "version": "0"}},
    }));
    send(serde_json::json!({"method": "initialized"}));
    for request in requests {
        wanted.push(request["id"].as_u64().expect("a request id"));
        send(request.clone());
    }

    let mut answers = BTreeMap::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wanted.iter().all(|id| answers.contains_key(id)) {
        let Some(line) = lines.next_before(deadline) else {
            let _ = child.kill();
            panic!("the app server did not answer {wanted:?} in time; got {answers:?}");
        };
        let Ok(reply) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = reply["id"].as_u64() else {
            continue;
        };
        assert!(reply.get("error").is_none(), "request {id} failed: {reply}");
        answers.insert(id, reply["result"].clone());
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    answers
}

fn hooks_list(codex: &Path, home: &Path, cwd: &Path) -> Vec<Value> {
    let answers = app_server(
        codex,
        home,
        cwd,
        &[serde_json::json!({"id": 2, "method": "hooks/list", "params": {}})],
    );
    let result = &answers[&2];
    result["data"][0]["hooks"]
        .as_array()
        .unwrap_or_else(|| panic!("no hooks in {result}"))
        .clone()
}

/// The fufu hooks by key, in `HOOK_KEYS` order, each asserted to be the
/// plugin's, enabled, and running `ff trigger codex`.
fn fufu_hooks(hooks: &[Value]) -> Vec<Value> {
    let mut keys: Vec<&str> = hooks
        .iter()
        .filter_map(|hook| hook["key"].as_str())
        .filter(|key| key.starts_with("fufu@fufu:"))
        .collect();
    keys.sort_unstable();
    let mut expected = HOOK_KEYS.to_vec();
    expected.sort_unstable();
    assert_eq!(keys, expected, "the five fufu hooks: {hooks:?}");
    HOOK_KEYS
        .iter()
        .map(|key| {
            let hook = hooks
                .iter()
                .find(|hook| hook["key"] == *key)
                .expect("the key was listed");
            assert_eq!(
                hook["command"],
                format!("\"{}\" trigger codex", ff_bin()),
                "{key}"
            );
            assert_eq!(hook["source"], "plugin", "{key}: {hook}");
            assert_eq!(hook["pluginId"], "fufu@fufu", "{key}: {hook}");
            assert_eq!(hook["enabled"], true, "{key}: {hook}");
            hook.clone()
        })
        .collect()
}

#[test]
fn codex_lists_the_five_hooks_and_the_plugin() {
    let Some(codex) = live_client("codex") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".codex");
    hook(&home, &fx.path(), "codex");

    let hooks = hooks_list(&codex, &home, &fx.path());
    for hook in fufu_hooks(&hooks) {
        assert_eq!(hook["trustStatus"], "untrusted", "{hook}");
        assert!(
            hook["currentHash"]
                .as_str()
                .is_some_and(|hash| hash.starts_with("sha256:")),
            "{hook}"
        );
    }
    let pre_tool = hooks
        .iter()
        .find(|hook| hook["key"] == HOOK_KEYS[0])
        .expect("the pre_tool_use hook");
    assert_eq!(pre_tool["matcher"], "Bash|apply_patch", "{pre_tool}");

    let answers = app_server(
        &codex,
        &home,
        &fx.path(),
        &[serde_json::json!({"id": 3, "method": "plugin/list", "params": {}})],
    );
    let result = &answers[&3];
    let marketplace = result["marketplaces"]
        .as_array()
        .unwrap_or_else(|| panic!("no marketplaces in {result}"))
        .iter()
        .find(|market| market["name"] == "fufu")
        .unwrap_or_else(|| panic!("no `fufu` marketplace in {result}"));
    let plugin = marketplace["plugins"]
        .as_array()
        .unwrap_or_else(|| panic!("no plugins in {marketplace}"))
        .iter()
        .find(|plugin| plugin["id"] == "fufu@fufu")
        .unwrap_or_else(|| panic!("no `fufu@fufu` in {marketplace}"));
    assert_eq!(plugin["installed"], true, "{plugin}");
    assert_eq!(plugin["enabled"], true, "{plugin}");
}

#[test]
fn a_codex_session_carries_the_briefing_and_its_hooks_capture() {
    let Some(codex) = live_client("codex") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".codex");
    hook(&home, &fx.path(), "codex");

    // Untrusted hooks are skipped without a word, so the session layer
    // starts by trusting each at the hash Codex reports for it — the
    // review `/hooks` would do by hand.
    let hooks = fufu_hooks(&hooks_list(&codex, &home, &fx.path()));
    let mut trust = String::new();
    for hook in &hooks {
        let key = hook["key"].as_str().unwrap();
        let hash = hook["currentHash"].as_str().unwrap();
        trust.push_str(&format!(
            "\n[hooks.state.\"{key}\"]\ntrusted_hash = \"{hash}\"\n"
        ));
    }
    let config = home.join(".codex").join("config.toml");
    let mut existing = std::fs::read_to_string(&config).unwrap_or_default();
    existing.push_str(&trust);
    std::fs::write(&config, existing).expect("write config.toml");
    for hook in fufu_hooks(&hooks_list(&codex, &home, &fx.path())) {
        assert_eq!(hook["trustStatus"], "trusted", "{hook}");
    }

    let commands = commands();
    let model = MockModel::start(
        &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        OP_LOG_MARKER,
    );
    let mut command = Command::new(&codex);
    client_env(&mut command, &home, &fx.path());
    command.env("MOCK_API_KEY", "sk-mock").args([
        "exec",
        "--json",
        "--skip-git-repo-check",
        // The first command writes into the worktree.
        "-s",
        "workspace-write",
        "-c",
        "model_provider=mock",
        "-c",
        "model_providers.mock.name=\"mock\"",
        "-c",
        &format!("model_providers.mock.base_url=\"{}/v1\"", model.base_url()),
        "-c",
        "model_providers.mock.env_key=\"MOCK_API_KEY\"",
        // The chat wire is refused by 0.154 ("no longer supported").
        "-c",
        "model_providers.mock.wire_api=\"responses\"",
        "-m",
        "gpt-5",
        "say hi",
    ]);
    let out = run_within(&mut command, Duration::from_secs(120));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "codex exec exited {:?}\nstdout: {stdout}",
        out.status.code()
    );

    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let thread_id = events
        .iter()
        .find(|event| event["type"] == "thread.started")
        .and_then(|event| event["thread_id"].as_str())
        .unwrap_or_else(|| panic!("no thread.started in {stdout}"))
        .to_string();
    let executions: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["type"] == "item.completed" && event["item"]["type"] == "command_execution"
        })
        .collect();
    assert_eq!(executions.len(), 2, "two commands ran: {stdout}");
    for execution in &executions {
        assert_eq!(execution["item"]["exit_code"], 0, "{execution}");
    }
    let output = executions[1]["item"]["aggregated_output"]
        .as_str()
        .unwrap_or_else(|| panic!("no aggregated_output in {}", executions[1]));
    // Codex's shell hook payload names the tool the way Claude Code's
    // does; the live run is what pins the spelling.
    captured_by(&op_log_from(output), "codex", &thread_id, "Bash");
    assert!(fx.path().join("b.txt").is_file(), "the first command ran");

    let requests = model.requests();
    let responses: Vec<&Recorded> = requests
        .iter()
        .filter(|request| request.path.ends_with("/v1/responses"))
        .collect();
    assert!(
        responses.len() >= 2,
        "two turns reach the model: {:?}",
        requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
    let first: Value = serde_json::from_str(&responses[0].body).expect("the first body parses");
    let developer = first["input"]
        .as_array()
        .unwrap_or_else(|| panic!("no input[] in {first}"))
        .iter()
        .filter(|item| item["role"] == "developer")
        .flat_map(|item| item["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|block| block["text"].as_str().map(str::to_string))
        .find(|text| text.starts_with(NOTICE_HEAD));
    assert!(
        developer.is_some(),
        "the briefing is a developer item in the first request: {first}"
    );
    let last = &responses[responses.len() - 1].body;
    assert!(last.contains("function_call_output"), "{last}");
    assert!(last.contains("op log"), "{last}");
}
