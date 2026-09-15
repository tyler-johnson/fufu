//! Qwen Code against the real binary: what it sends once a session runs
//! after `ff hook qwen`.
//!
//! The layer above `tests/installers.rs`, which proves what fufu writes
//! and not what the client reads. Qwen Code has no verb that lists its
//! hooks, so the settings file stays the contract
//! (`installers.rs::qwen_is_wired_in_its_settings_file`) and the
//! model-free layer is nothing beyond the binary answering. The session
//! layer is a headless `qwen -p` against `ff_testsupport::mock_model`,
//! where the proof is what Qwen Code sent: the briefing in the first
//! request, and the `ff op log` output carried back, whose top entry is
//! a capture the client's hooks took under the session. Not the
//! `PreToolUse` capture itself: Qwen Code 0.23.3 continues after a tool
//! result by firing `UserPromptSubmit` with an empty prompt ahead of the
//! next tool call, so that event sees the moved tree first and the tool
//! event's capture is a no-op. `PreToolUse` under the matcher in
//! `qwen.rs` was verified by hand against 0.23.3 with a recording hook:
//! it fires for `run_shell_command` with the family payload
//! (`tool_name`, `tool_input.command`, `cwd`, `session_id`).
//!
//! Needs `qwen` on `PATH`. Skips with a line when it is absent, and fails
//! instead under `FF_LIVE=1`, which is how CI runs it.

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
fn a_qwen_session_carries_the_briefing_and_its_hooks_capture() {
    let Some(qwen) = live_client("qwen") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".qwen");
    hook(&home, &fx.path(), "qwen");

    // The settings name the bare `ff trigger qwen`; `client_env` puts this
    // build's binary first on PATH for it.
    let commands = commands();
    let model = MockModel::start(
        &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        OP_LOG_MARKER,
    );
    let mut command = Command::new(&qwen);
    client_env(&mut command, &home, &fx.path());
    command.args([
        "--auth-type",
        "openai",
        "--openai-base-url",
        &format!("{}/v1", model.base_url()),
        "--openai-api-key",
        "sk-mock",
        "-m",
        "mock",
        "--output-format",
        "json",
        // Headless Qwen Code classifies `touch` as an edit and denies it
        // under every mode but this one; `--allowed-tools` does not
        // override the deny rule.
        "--approval-mode",
        "yolo",
        // `-p` rather than a positional prompt: with stdin closed the
        // positional form fails on "No input provided via stdin".
        "-p",
        "say hi",
    ]);
    let out = run_within(&mut command, Duration::from_secs(120));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "qwen -p exited {:?}\nstdout: {stdout}",
        out.status.code()
    );

    // One JSON array of events; anything the client prints ahead of it is
    // skipped to the first bracket.
    let start = stdout
        .find('[')
        .unwrap_or_else(|| panic!("no event array in {stdout}"));
    let events: Vec<Value> = serde_json::Deserializer::from_str(&stdout[start..])
        .into_iter()
        .next()
        .expect("an array")
        .unwrap_or_else(|err| panic!("the events do not parse: {err}\n{stdout}"));
    let session_id = events
        .iter()
        .find(|event| event["type"] == "system" && event["subtype"] == "init")
        .and_then(|event| event["session_id"].as_str())
        .unwrap_or_else(|| panic!("no init event in {stdout}"))
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
    captured_under(&op_log_from(&results[1]), "qwen", &session_id);
    assert!(fx.path().join("b.txt").is_file(), "the first command ran");

    // First and any, never a count: Qwen Code may extract memories with
    // one more request on its way out.
    let requests = model.requests();
    let chats: Vec<_> = requests
        .iter()
        .filter(|request| request.path.ends_with("/v1/chat/completions"))
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
