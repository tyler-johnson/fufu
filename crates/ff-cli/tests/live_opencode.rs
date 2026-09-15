//! OpenCode against the real binary: what it sends once a session runs
//! after `ff hook opencode`.
//!
//! The layer above `tests/installers.rs`, which proves what fufu writes
//! and not what the client loads. OpenCode loads every plugin module
//! under its config directory at startup, so the proof is a headless
//! `opencode run` against `ff_testsupport::mock_model` through a custom
//! provider in `opencode.json`: the standing briefing in the first
//! request's system prompt, and the `ff op log` output carried back on
//! the next turn, whose top entry is a capture the plugin's hooks took
//! under the session OpenCode named. Not `tool.execute.before`'s own:
//! the system-prompt transform runs on every model call, so its
//! `SessionStart` sees the moved tree ahead of the next tool call and
//! the tool event's capture is a no-op. `tool.execute.before` was
//! verified by hand against 1.18.30 with a recording trigger: it sends
//! `tool_name: "bash"` and `tool_input.command`, which is what the
//! plugin's `output.args` reading was written for. No model judges
//! anything.
//!
//! Needs `opencode` on `PATH`. Skips with a line when it is absent, and
//! fails instead under `FF_LIVE=1`, which is how CI runs it. The first
//! run installs `@ai-sdk/openai-compatible` under the scratch cache —
//! network, once — so the deadline is generous.

mod support;

use std::process::Command;
use std::time::Duration;

use ff_testsupport::mock_model::MockModel;
use serde_json::Value;
use support::{
    NOTICE_HEAD, OP_LOG_MARKER, captured_under, client_env, commands, hook, live_client,
    op_log_from, repo, run_within, scratch_home,
};

#[test]
fn an_opencode_session_carries_the_briefing_and_its_hooks_capture() {
    let Some(opencode) = live_client("opencode") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".config/opencode");
    // The adapter honors `XDG_CONFIG_HOME`, which `client_env` and `hook`
    // both point at `home/xdg`, so the plugin lands where the client
    // under the same environment reads it.
    hook(&home, &fx.path(), "opencode");
    let config = home.join("xdg").join("opencode");
    assert!(config.join("plugins").join("fufu.js").is_file());

    // The mock as a custom provider: the OpenAI-compatible package posts
    // to `/v1/chat/completions`, the dialect the mock speaks for Qwen.
    let commands = commands();
    let model = MockModel::start(
        &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        OP_LOG_MARKER,
    );
    let provider = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "mock": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "mock",
                "options": { "baseURL": format!("{}/v1", model.base_url()), "apiKey": "sk-mock" },
                "models": { "mock": { "name": "mock" } }
            }
        }
    });
    std::fs::write(
        config.join("opencode.json"),
        serde_json::to_string_pretty(&provider).unwrap(),
    )
    .unwrap();

    let mut command = Command::new(&opencode);
    client_env(&mut command, &home, &fx.path());
    command.args([
        "run",
        "--model",
        "mock/mock",
        "--format",
        "json",
        "--dangerously-skip-permissions",
        "say hi",
    ]);
    let out = run_within(&mut command, Duration::from_secs(180));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "opencode run exited {:?}\nstdout: {stdout}",
        out.status.code()
    );

    // One JSON object per line; anything else the client prints is
    // skipped.
    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect();
    assert!(!events.is_empty(), "no JSON events in {stdout}");
    let errors: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] == "error")
        .collect();
    assert!(errors.is_empty(), "the client reported errors: {errors:?}");
    let session_id = events
        .iter()
        .find_map(|event| event["sessionID"].as_str())
        .unwrap_or_else(|| panic!("no sessionID in {stdout}"))
        .to_string();
    let output = events
        .iter()
        .filter(|event| event["type"] == "tool_use")
        .find_map(|event| {
            let text = match &event["part"]["state"]["output"] {
                Value::String(text) => text.clone(),
                _ => event.to_string(),
            };
            text.contains(OP_LOG_MARKER).then_some(text)
        })
        .unwrap_or_else(|| panic!("no tool_use carrying the op log envelope in {stdout}"));
    captured_under(&op_log_from(&output), "opencode", &session_id);
    assert!(fx.path().join("b.txt").is_file(), "the first command ran");

    // The standing briefing is in the first request's system prompt, and
    // a later request carries the tool's output.
    let requests = model.requests();
    let chats: Vec<_> = requests
        .iter()
        .filter(|request| request.path.ends_with("/chat/completions"))
        .collect();
    assert!(
        chats.len() >= 2,
        "two turns reach the model: {:?}",
        requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
    assert!(
        chats[0].body.contains(NOTICE_HEAD),
        "the briefing is in the first request: {}",
        chats[0].body
    );
    let carried = chats.iter().skip(1).any(|request| {
        let body: Value = serde_json::from_str(&request.body).expect("a body parses");
        body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| message["role"] == "tool" && message.to_string().contains("op log"))
    });
    assert!(carried, "a later request carries the tool's output");
}
