//! Gemini CLI.
//!
//! Names the same payload fields as Claude Code and Codex, with its own
//! event and tool vocabulary that the neutral `EventKind` already absorbs.
//! What it cannot do is read plain text: injected context has to arrive as
//! JSON, which is why the briefing envelope is asked of the adapter instead
//! of assumed.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Status,
    Wiring, payload, settings,
};

pub struct Gemini;

const COMMAND: &str = "ff trigger gemini";
const LEGACY: [&str; 0] = [];

const EVENTS: [(&str, Option<&str>); 2] = [
    ("BeforeTool", Some("run_shell_command|write_file|replace")),
    ("SessionStart", None),
];

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".gemini"))
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

impl Integration for Gemini {
    fn slug(&self) -> &'static str {
        "gemini"
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
            Err(err) => Wiring::Unavailable(err.to_string()),
        };
        let stale = spec().map(|spec| settings::stale(&spec)).unwrap_or(false);
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note: None,
            parts: Vec::new(),
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
        Some(&Gemini)
    }
}

impl AgentProtocol for Gemini {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// Gemini reads injected context out of a JSON field, so plain stdout
    /// would be discarded — and discarded silently, which is the worst of
    /// the available failures.
    fn briefing_envelope(&self, text: &str) -> String {
        serde_json::json!({
            "hookSpecificOutput": { "additionalContext": text }
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    #[test]
    fn the_recorded_payload_maps_geminis_own_event_names() {
        let event = Gemini
            .parse(
                br#"{"hook_event_name":"BeforeTool","session_id":"g-1","cwd":"/repo",
                     "tool_name":"run_shell_command","tool_input":{"command":"ls -la"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.label, Label::text("run_shell_command(ls -la)"));

        let event = Gemini
            .parse(
                br#"{"hook_event_name":"SessionStart","session_id":"g-1","cwd":"/repo"}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::ContextStart);
    }

    #[test]
    fn the_briefing_is_json_wrapped() {
        let out = Gemini.briefing_envelope("hello");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "hello");
    }
}
