//! Qwen Code, the Gemini CLI fork.
//!
//! This adapter was Gemini CLI's. Gemini CLI's hook names have since
//! diverged from the family (`BeforeAgent`, `AfterAgent`), while Qwen
//! Code, the fork with users, kept the inherited shape: hooks in
//! `~/.qwen/settings.json` under Claude's event names, the same payload
//! fields, and injected context read out of
//! `hookSpecificOutput.additionalContext`. So the body of the adapter is
//! the fork's inheritance, re-pointed at the fork, and Gemini went. An
//! existing `~/.gemini/settings.json` is left as found; `ff trigger
//! gemini` is a stored spelling and keeps answering from `retired.rs`.
//!
//! What Qwen cannot do is read plain text: injected context has to arrive
//! as JSON, which is why the briefing envelope is asked of the adapter
//! instead of assumed. Whether it reads a skills directory is unknown, so
//! it gets the briefing alone.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, payload, settings,
};
use settings::{Event, Need};

pub struct Qwen;

/// The hook command. No older spelling: nothing before this adapter ever
/// wrote `~/.qwen/settings.json`.
const COMMAND: &str = "ff trigger qwen";
const LEGACY: [&str; 0] = [];

/// The family's five, in Qwen's own hook enum. `PreToolUse` is the one
/// capture cannot miss, on the tools that write; `UserPromptSubmit` is the
/// turn the briefing rides. The rest widen capture rather than found it.
const EVENTS: [Event; 5] = [
    (
        "PreToolUse",
        Some("run_shell_command|write_file|replace|edit"),
        Need::Required,
    ),
    ("UserPromptSubmit", None, Need::Required),
    ("SessionStart", None, Need::Extra),
    ("Stop", None, Need::Extra),
    ("SessionEnd", None, Need::Extra),
];

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".qwen"))
}

fn spec() -> Result<settings::Spec> {
    Ok(settings::Spec {
        path: config_dir()?.join("settings.json"),
        shape: settings::Shape::Nested,
        events: &EVENTS,
        command: COMMAND.into(),
        legacy: &LEGACY,
        version: None,
    })
}

impl Integration for Qwen {
    fn slug(&self) -> &'static str {
        "qwen"
    }

    fn detect(&self) -> Presence {
        match config_dir() {
            Ok(dir) if dir.is_dir() => Presence::Present { evidence: dir },
            _ => Presence::Absent,
        }
    }

    fn status(&self) -> Status {
        let wiring = match spec() {
            Ok(spec) => settings::wiring(&spec),
            Err(err) => Wiring::Unavailable {
                complaint: err.to_string(),
            },
        };
        let stale = spec().map(|spec| settings::stale(&spec)).unwrap_or(false);
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note: None,
            parts: Vec::new(),
            skill: None,
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        settings::install(&spec()?)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        settings::uninstall(&spec()?)
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Qwen)
    }
}

impl AgentProtocol for Qwen {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// Qwen reads injected context out of a JSON field, so plain stdout
    /// would be discarded — and discarded silently, which is the worst of
    /// the available failures. On a tool it documents no channel at all,
    /// so nothing is said there.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool || reply.context.is_empty() {
            return None;
        }
        Some(
            serde_json::json!({
                "hookSpecificOutput": { "additionalContext": reply.joined() }
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    #[test]
    fn the_recorded_payload_parses_the_family_shape() {
        let event = Qwen
            .parse(
                br#"{"hook_event_name":"PreToolUse","session_id":"q-1","cwd":"/repo",
                     "tool_name":"run_shell_command","tool_input":{"command":"ls -la"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "q-1");
        assert_eq!(event.label, Label::text("run_shell_command(ls -la)"));

        let event = Qwen
            .parse(
                br#"{"hook_event_name":"SessionStart","session_id":"q-1","cwd":"/repo"}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::SessionStart);
    }

    #[test]
    fn the_briefing_is_json_wrapped_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::ContextStart);
        reply.context.push("hello".into());
        let out = Qwen.reply_envelope(&reply).expect("a turn speaks");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "hello");

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Qwen.reply_envelope(&reply).is_none());
    }
}
