//! Claude Code against the real binary: what it loads after `ff hook
//! claude`, and what it sends once a session runs.
//!
//! The layer above `tests/installers.rs`, which proves what fufu writes
//! and not what the client reads. Here the client lists its plugins with
//! no session started, and a headless `claude -p` runs against
//! `ff_testsupport::mock_model`, where the proof is what Claude Code sent:
//! the briefing in the first request, and the `ff op log` output carried
//! back on the next turn, whose top entry is the capture the client's
//! hooks took under the session before the tool ran. No model judges
//! anything.
//!
//! Needs `claude` on `PATH`. Skips with a line when it is absent, and
//! fails instead under `FF_LIVE=1`, which is how CI runs it.

mod support;

use std::process::Command;
use std::time::Duration;

use ff_testsupport::mock_model::MockModel;
use serde_json::Value;
use support::{
    NOTICE_HEAD, OP_LOG_MARKER, TOUCH, captured_by, client_env, commands, hook, live_client,
    op_log_from, repo, run_within, scratch_home,
};

#[test]
fn claude_lists_the_plugin() {
    let Some(claude) = live_client("claude") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".claude");
    hook(&home, &fx.path(), "claude");

    let mut command = Command::new(&claude);
    client_env(&mut command, &home, &fx.path());
    command.args(["plugin", "list"]);
    let out = run_within(&mut command, Duration::from_secs(60));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "claude plugin list exited {:?}\nstdout: {stdout}",
        out.status.code()
    );
    assert!(stdout.contains("fufu@skills-dir"), "{stdout}");
    assert!(stdout.contains("loaded"), "{stdout}");
}

#[test]
fn a_claude_session_carries_the_briefing_and_its_hooks_capture() {
    let Some(claude) = live_client("claude") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".claude");
    hook(&home, &fx.path(), "claude");

    let commands = commands();
    let model = MockModel::start(
        &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        OP_LOG_MARKER,
    );
    let mut command = Command::new(&claude);
    client_env(&mut command, &home, &fx.path());
    command
        .env("ANTHROPIC_BASE_URL", model.base_url())
        .env("ANTHROPIC_API_KEY", "sk-mock")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .args([
            "-p",
            "say hi",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model",
            "claude-sonnet-5",
            // The exact commands are allowed and nothing else: no bypass.
            "--allowedTools",
            &format!("Bash({TOUCH}),Bash({})", commands[1]),
        ]);
    let out = run_within(&mut command, Duration::from_secs(120));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "claude -p exited {:?}\nstdout: {stdout}",
        out.status.code()
    );

    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let session_id = events
        .iter()
        .find(|event| event["type"] == "result")
        .and_then(|event| event["session_id"].as_str())
        .unwrap_or_else(|| panic!("no result event in {stdout}"))
        .to_string();
    let results: Vec<String> = events
        .iter()
        .filter(|event| event["type"] == "user")
        .flat_map(|event| {
            event["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|block| block["type"] == "tool_result")
        .map(|result| match &result["content"] {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            other => panic!("tool_result content is {other}"),
        })
        .collect();
    assert_eq!(results.len(), 2, "two tool calls ran: {stdout}");
    captured_by(&op_log_from(&results[1]), "claude", &session_id, "Bash");
    assert!(fx.path().join("b.txt").is_file(), "the first command ran");

    let requests = model.requests();
    let messages: Vec<_> = requests
        .iter()
        .filter(|request| request.path.contains("/v1/messages"))
        .collect();
    assert!(
        messages.len() >= 2,
        "two turns reach the model: {:?}",
        requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
    let first: Value = serde_json::from_str(&messages[0].body).expect("the first body parses");
    let briefing = first["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("no messages[] in {first}"))
        .iter()
        .filter(|message| message["role"] == "system")
        .flat_map(|message| match &message["content"] {
            Value::String(text) => vec![text.clone()],
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| block["text"].as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        })
        .find(|text| text.contains(NOTICE_HEAD));
    assert!(
        briefing.is_some(),
        "the briefing is a system message in the first request: {first}"
    );
    let last = &messages[messages.len() - 1].body;
    assert!(last.contains("tool_result"), "{last}");
    assert!(last.contains("op log"), "{last}");
}
