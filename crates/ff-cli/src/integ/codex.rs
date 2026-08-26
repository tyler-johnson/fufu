//! Codex.
//!
//! Field-for-field compatible with Claude Code's payload, down to aliasing
//! `CLAUDE_PLUGIN_ROOT`, so it shares the payload dialect and differs only
//! in where its config lives and which tools its matcher names.
//!
//! The one thing that needs saying out loud: Codex records trust against a
//! hook's hash and skips new or changed hooks until they are reviewed
//! through `/hooks`. Without that told, capture silently never happens and
//! nothing explains why — which is the exact failure a capture floor exists
//! to make impossible.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Status,
    Wiring, payload, settings,
};

pub struct Codex;

const COMMAND: &str = "ff trigger codex";
const LEGACY: [&str; 0] = [];

const EVENTS: [(&str, Option<&str>); 2] = [
    ("PreToolUse", Some("Bash|apply_patch")),
    ("UserPromptSubmit", None),
];

const TRUST: &str = "Codex trusts a hook by its hash: run /hooks in Codex to review this one, \
                     or it is skipped and nothing captures";

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".codex"))
}

fn spec() -> Result<settings::Spec> {
    Ok(settings::Spec {
        path: config_dir()?.join("hooks.json"),
        shape: settings::Shape::Nested,
        events: &EVENTS,
        command: COMMAND.into(),
        legacy: &LEGACY,
        version: None,
    })
}

impl Integration for Codex {
    fn slug(&self) -> &'static str {
        "codex"
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
            // The trust step is news whenever the wiring is there: an
            // unreviewed hook and a missing one look identical from here.
            note: wiring.feeds_capture().then(|| TRUST.to_string()),
            wiring,
            parts: Vec::new(),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::install(&spec()?)?;
        change.lines.push(TRUST.into());
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        settings::uninstall(&spec()?)
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Codex)
    }
}

impl AgentProtocol for Codex {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// Plain stdout, the same as Claude Code.
    fn briefing_envelope(&self, text: &str) -> String {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    #[test]
    fn the_recorded_payload_parses_the_same_as_claudes() {
        let event = Codex
            .parse(
                br#"{"hook_event_name":"PreToolUse","session_id":"cx-1","cwd":"/repo",
                     "tool_name":"apply_patch","tool_input":{"file_path":"/repo/a.rs"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "cx-1");
        assert_eq!(
            event.label,
            Label::Path {
                tool: "apply_patch".into(),
                path: "/repo/a.rs".into()
            }
        );
    }

    #[test]
    fn the_briefing_goes_out_as_plain_text() {
        assert_eq!(Codex.briefing_envelope("hello"), "hello");
    }
}
