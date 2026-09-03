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
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, mcp, payload, settings,
};
use settings::Need;

pub struct Gemini;

const COMMAND: &str = "ff trigger gemini";
const LEGACY: [&str; 0] = [];

const EVENTS: [(&str, Option<&str>, Need); 2] = [
    (
        "BeforeTool",
        Some("run_shell_command|write_file|replace"),
        Need::Required,
    ),
    ("SessionStart", None, Need::Required),
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

/// The MCP server goes in the same file as the hooks, under its own key;
/// Gemini spells no transport, so the entry carries none.
fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec::new(
        config_dir()?.join("settings.json"),
        mcp::Shape::Json { with_type: false },
    ))
}

/// Every declared extension's own server in `settings.json`, and every
/// name registered there that nothing declares any more.
fn mcp_ext_status() -> (Vec<mcp::McpExtension>, Vec<String>) {
    match mcp_spec() {
        Ok(spec) => mcp::extensions(&spec),
        Err(_) => (Vec::new(), Vec::new()),
    }
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
        let (mcp_extensions, mcp_orphaned) = mcp_ext_status();
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note: None,
            parts: Vec::new(),
            skill: None,
            mcp: Some(match mcp_spec() {
                Ok(spec) => mcp::wiring(&spec),
                Err(err) => Wiring::Unavailable(err.to_string()),
            }),
            mcp_extensions,
            mcp_orphaned,
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::install(&spec()?)?;
        change.absorb(mcp::install(&mcp_spec()?)?);
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::uninstall(&spec()?)?;
        change.absorb(mcp::uninstall(&mcp_spec()?)?);
        Ok(change)
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
        // Gemini fires this once per session, so a boundary event is what
        // it has always been for it.
        assert_eq!(event.kind, EventKind::SessionStart);
    }

    #[test]
    fn the_briefing_is_json_wrapped_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::SessionStart);
        reply.context.push("hello".into());
        let out = Gemini
            .reply_envelope(&reply)
            .expect("a session start speaks");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "hello");

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Gemini.reply_envelope(&reply).is_none());
    }
}
