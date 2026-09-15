//! The trigger sources fufu no longer writes and answers forever.
//!
//! A source name ends up inside a config file fufu does not own and
//! rewrites only when somebody runs the installer again, which they may
//! never do. So a spelling once written is accepted for good, at the cost
//! of an entry here: `ff trigger gemini` from a `~/.gemini/settings.json`
//! nobody cleaned keeps capturing under the source `gemini`, and keeps
//! briefing in the envelope it was written with. A retired source carries
//! its protocol and nothing else: no slug, no detection, no install, and
//! `ff hook gemini` is the unknown-slug refusal.
//!
//! Gemini CLI went because its hook names diverged from the family
//! (`BeforeAgent`, `AfterAgent`) while Qwen Code, the fork with users,
//! kept the inherited shape; `qwen.rs` is the adapter's successor.

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, payload,
};

pub struct Retired {
    source: &'static str,
    proto: &'static dyn AgentProtocol,
}

/// Gemini CLI's dialect: the shared payload, its own event names
/// (`BeforeTool`, `SessionStart`) already absorbed by `EventKind`, and
/// injected context read out of `hookSpecificOutput.additionalContext`.
/// No channel on a tool.
struct GeminiProto;

impl AgentProtocol for GeminiProto {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

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

static GEMINI_PROTO: GeminiProto = GeminiProto;

static GEMINI: Retired = Retired {
    source: "gemini",
    proto: &GEMINI_PROTO,
};

/// Every retired source, for the tests that walk them.
pub fn all() -> [&'static Retired; 1] {
    [&GEMINI]
}

/// The retired source a stored spelling names, if it is one.
pub fn by_source(source: &str) -> Option<&'static dyn Integration> {
    all()
        .into_iter()
        .find(|retired| retired.source == source)
        .map(|retired| retired as &dyn Integration)
}

impl Integration for Retired {
    fn slug(&self) -> &'static str {
        self.source
    }

    fn detect(&self) -> Presence {
        Presence::Absent
    }

    fn status(&self) -> Status {
        Status {
            slug: self.source,
            presence: Presence::Absent,
            wiring: Wiring::NotWired,
            note: None,
            parts: Vec::new(),
            skill: None,
            stale: false,
        }
    }

    /// Unreachable through `by_slug`, which never sees a retired source;
    /// the refusal is spelled once, in `verbs.rs`.
    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        Err(super::verbs::unknown_slug(self.source, "hook"))
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        Err(super::verbs::unknown_slug(self.source, "unhook"))
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(self.proto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    /// The recorded payload, as Gemini CLI sent it, still lands the way it
    /// did — its own event names and no channel on a tool.
    #[test]
    fn gemini_keeps_its_event_names_and_its_envelope() {
        let gemini = by_source("gemini").expect("a retired source");
        let proto = gemini.protocol().expect("carries its protocol");
        let event = proto
            .parse(
                br#"{"hook_event_name":"BeforeTool","session_id":"g-1","cwd":"/repo",
                     "tool_name":"run_shell_command","tool_input":{"command":"ls -la"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.label, Label::text("run_shell_command(ls -la)"));

        let mut reply = Reply::new(EventKind::SessionStart);
        reply.context.push("hello".into());
        let out = proto
            .reply_envelope(&reply)
            .expect("a session start speaks");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "hello");

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(proto.reply_envelope(&reply).is_none());

        for live in ["claude", "codex", "cursor", "qwen", "shell"] {
            assert!(by_source(live).is_none(), "{live} is a live source");
        }
    }

    /// Retired means unhookable: no presence, no wiring, and the install
    /// path is the unknown-slug refusal.
    #[test]
    fn a_retired_source_cannot_be_hooked() {
        for retired in all() {
            assert_eq!(retired.detect(), Presence::Absent, "{}", retired.source);
            assert_eq!(retired.status().wiring, Wiring::NotWired);
            let Err(err) = retired.install(&InstallOptions::default()) else {
                panic!("{} installs nothing", retired.source);
            };
            assert_eq!(err.id(), "usage/unknown-slug", "{}", retired.source);
            let Err(err) = retired.uninstall(&InstallOptions::default()) else {
                panic!("{} uninstalls nothing", retired.source);
            };
            assert_eq!(err.id(), "usage/unknown-slug", "{}", retired.source);
        }
    }
}
