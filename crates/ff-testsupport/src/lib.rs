pub mod capture;
pub mod fixtures;
pub mod hooks;
pub mod mock_model;
pub mod paths;
pub mod porcelain;
pub mod scenarios;
pub mod userdirs;

pub use fixtures::{Fixture, legacy_park};
pub use scenarios::scenarios;

/// Every variable an agent client sets in the processes it starts — the
/// session it is running, and the marker that says which client it is —
/// plus fufu's own session tag and Copilot's event variable. A suite run
/// under an agent would otherwise stamp the developer's session on every
/// fixture capture.
pub const AGENT_ENV: &[&str] = &[
    "FF_SESSION",
    "FF_HOOK_EVENT",
    "CLAUDE_CODE_SESSION_ID",
    "CODEX_SESSION_ID",
    "QWEN_CODE_SESSION_ID",
    "OPENCODE_SESSION_ID",
    "COPILOT_AGENT_SESSION_ID",
    "CURSOR_CONVERSATION_ID",
    "CLAUDECODE",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    "CODEX_SANDBOX",
    "CURSOR_AGENT",
    "QWEN_CODE",
    "OPENCODE",
    "COPILOT_CLI",
];

/// Remove every [`AGENT_ENV`] variable from a spawn, so the captures a
/// harness takes are stamped by what the test sets and nothing else.
pub fn scrub(command: &mut std::process::Command) {
    for name in AGENT_ENV {
        command.env_remove(name);
    }
}
