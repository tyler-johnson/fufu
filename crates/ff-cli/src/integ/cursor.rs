//! Cursor's agent client.
//!
//! `cursor` is the agent, not the editor: a future editor integration gets
//! a slug of its own, because these slugs end up inside config files and
//! cannot be renamed afterward.
//!
//! Two things here differ from the other three. The config file is flatter
//! — an entry *is* a command, with no nested list — and the session id
//! moves: tool events carry `conversation_id` while `sessionStart` carries
//! `session_id`. Both are read, so a session is one id either way and the
//! briefing marker does not thrash.
//!
//! `sessionStart` does not fire for cloud agents, so the briefing is simply
//! absent there. Capture still works, because it rides `preToolUse`. That
//! is reported rather than papered over.

use std::path::PathBuf;

use ff_core::Result;
use serde::Deserialize;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, mcp, payload, settings,
};
use settings::Need;

pub struct Cursor;

const COMMAND: &str = "ff trigger cursor";
const LEGACY: [&str; 0] = [];

const EVENTS: [(&str, Option<&str>, Need); 2] = [
    ("preToolUse", Some("Shell|Write|Delete"), Need::Required),
    ("sessionStart", None, Need::Required),
];

const CLOUD: &str = "Cursor does not fire sessionStart for cloud agents, so the briefing is \
                     absent there — capture still rides preToolUse";

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".cursor"))
}

fn spec() -> Result<settings::Spec> {
    Ok(settings::Spec {
        path: config_dir()?.join("hooks.json"),
        shape: settings::Shape::Flat,
        events: &EVENTS,
        command: COMMAND.into(),
        legacy: &LEGACY,
        version: Some(1),
    })
}

/// The MCP server goes in a file of its own, `mcp.json`, which is where
/// Cursor reads servers from; the hooks file is not it.
fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec::new(
        config_dir()?.join("mcp.json"),
        mcp::Shape::Json { with_type: true },
    ))
}

/// Cursor's payload. Same idea as the shared dialect, different names for
/// the one field that matters most.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Payload {
    hook_event_name: String,
    /// Tool events carry this one.
    conversation_id: String,
    /// `sessionStart` carries this one.
    session_id: String,
    cwd: String,
    tool_name: String,
    tool_input: payload::ToolInput,
    prompt: String,
}

impl Integration for Cursor {
    fn slug(&self) -> &'static str {
        "cursor"
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
            note: wiring.feeds_capture().then(|| CLOUD.to_string()),
            wiring,
            parts: Vec::new(),
            skill: None,
            mcp: Some(match mcp_spec() {
                Ok(spec) => mcp::wiring(&spec),
                Err(err) => Wiring::Unavailable(err.to_string()),
            }),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::install(&spec()?)?;
        change.absorb(mcp::install(&mcp_spec()?)?);
        change.lines.push(CLOUD.into());
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::uninstall(&spec()?)?;
        change.absorb(mcp::uninstall(&mcp_spec()?)?);
        Ok(change)
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Cursor)
    }
}

impl AgentProtocol for Cursor {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let raw: Payload = payload::parse_json(stdin)?;
        // Whichever id this event happens to carry. One session must read
        // as one session across both channels, or the briefing marker
        // flips on every turn and the agent is briefed forever.
        let session = if raw.conversation_id.is_empty() {
            raw.session_id
        } else {
            raw.conversation_id
        };
        // `sessionStart` has no cwd; the client's own working directory is
        // the honest fallback, and it is where the session was started.
        let cwd = if raw.cwd.is_empty() {
            std::env::current_dir()
                .map_err(ff_core::Error::repo)?
                .display()
                .to_string()
        } else {
            raw.cwd
        };
        let kind = forced
            .or_else(|| EventKind::from_hint(&raw.hook_event_name))
            .unwrap_or(EventKind::Other);
        let label = match kind {
            EventKind::BeforeTool => payload::tool_label(&raw.tool_name, &raw.tool_input),
            _ => payload::event_label(kind, &raw.hook_event_name, &raw.prompt),
        };
        Ok(Some(AgentEvent {
            kind,
            session,
            // Cursor's payload names no subagent, so every event is the
            // main thread's.
            agent: String::new(),
            cwd: cwd.into(),
            label,
            command: payload::command_of(&raw.tool_input),
        }))
    }

    /// Cursor takes injected context as a JSON field, the way Gemini does,
    /// under its own name — and documents no channel on a tool, so nothing
    /// is said there.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool || reply.context.is_empty() {
            return None;
        }
        Some(serde_json::json!({ "additional_context": reply.joined() }).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    #[test]
    fn a_tool_event_reads_the_conversation_id() {
        let event = Cursor
            .parse(
                br#"{"hook_event_name":"preToolUse","conversation_id":"conv-9","cwd":"/repo",
                     "tool_name":"Shell","tool_input":{"command":"npm test"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "conv-9");
        assert_eq!(event.label, Label::text("Shell(npm test)"));
    }

    /// Both channels have to agree on the session, or the briefing marker
    /// flips on every turn.
    #[test]
    fn session_start_reads_the_session_id() {
        let event = Cursor
            .parse(
                br#"{"hook_event_name":"sessionStart","session_id":"conv-9","cwd":"/repo"}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::SessionStart);
        assert_eq!(event.session, "conv-9");
    }

    #[test]
    fn the_briefing_is_json_wrapped_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::SessionStart);
        reply.context.push("hello".into());
        let out = Cursor
            .reply_envelope(&reply)
            .expect("a session start speaks");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["additional_context"], "hello");

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Cursor.reply_envelope(&reply).is_none());
    }
}
