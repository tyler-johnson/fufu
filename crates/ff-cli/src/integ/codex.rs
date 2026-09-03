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
//! to make impossible. The trust step gates the hook and nothing else: the
//! shipped skill is a file Codex reads, not a command it runs.
//!
//! Three mechanisms, then, and they are independent. The hooks are entries
//! merged into a settings file that belongs to the user; the skill is a
//! directory fufu owns outright under `~/.codex/skills/`, written whole and
//! removed whole; the MCP server is a marked block in `config.toml`, the
//! one TOML file among the four clients, appended and removed by its
//! markers. No install can take another down with it.
//!
//! A declared extension's own skill files take a directory of their own
//! beside fufu's, `~/.codex/skills/<name>/`, on the same written-whole,
//! removed-whole rule. Namespaced by the extension's name rather than
//! nested inside a directory fufu owns outright the way Claude's plugin
//! makes possible, so a name collision with something already living under
//! `~/.codex/skills/` is a risk this mechanism accepts rather than one it
//! can detect.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Presence, Reply,
    Status, Wiring, mcp, payload, settings, skill,
};
use settings::Need;

pub struct Codex;

const COMMAND: &str = "ff trigger codex";
const LEGACY: [&str; 0] = [];

const EVENTS: [(&str, Option<&str>, Need); 2] = [
    ("PreToolUse", Some("Bash|apply_patch"), Need::Required),
    ("UserPromptSubmit", None, Need::Required),
];

const TRUST: &str = "Codex trusts a hook by its hash: run /hooks in Codex to review this one, \
                     or it is skipped and nothing captures";

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".codex"))
}

fn skill_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("skills").join(skill::NAME))
}

fn skill_wiring() -> Wiring {
    match skill_dir() {
        Ok(dir) => skill::wiring(&dir),
        Err(_) => Wiring::NotWired,
    }
}

/// A declared extension's own directory beside `skills/fufu`, under
/// `~/.codex/skills/`. Namespaced by the extension's own name, the same
/// namespace everything else about it hangs off.
fn ext_skill_dir(name: &str) -> Result<PathBuf> {
    Ok(config_dir()?.join("skills").join(name))
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

fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec::new(
        config_dir()?.join("config.toml"),
        mcp::Shape::TomlBlock,
    ))
}

/// Every declared extension's own table in the marked block, and every
/// table there that nothing declares any more.
fn mcp_ext_status() -> (Vec<mcp::McpExtension>, Vec<String>) {
    match mcp_spec() {
        Ok(spec) => mcp::extensions(&spec),
        Err(_) => (Vec::new(), Vec::new()),
    }
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
        let (mcp_extensions, mcp_orphaned) = mcp_ext_status();
        Status {
            slug: self.slug(),
            presence: self.detect(),
            // The trust step is news whenever the wiring is there: an
            // unreviewed hook and a missing one look identical from here.
            note: wiring.feeds_capture().then(|| TRUST.to_string()),
            wiring,
            parts: Vec::new(),
            skill: Some(skill_wiring()),
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
        let dir = skill_dir()?;
        skill::write(&dir)?;
        change.absorb(Change::changed(format!(
            "skill written to {}",
            dir.display()
        )));
        for declared in crate::registry::read().declared() {
            let ext_dir = ext_skill_dir(declared.name())?;
            if skill::write_sources(&ext_dir, declared)? > 0 {
                change.absorb(Change::changed(format!(
                    "{} skill written to {}",
                    declared.name(),
                    ext_dir.display()
                )));
            }
        }
        change.absorb(mcp::install(&mcp_spec()?)?);
        change.lines.push(TRUST.into());
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let mut change = settings::uninstall(&spec()?)?;
        let dir = skill_dir()?;
        if skill::remove(&dir)? {
            change.absorb(Change::changed(format!("removed {}", dir.display())));
        }
        // Only the extensions still declared: an extension taken back with
        // `ff extension remove` before this runs leaves its own directory
        // behind, the same way its manifest's other traces do once nothing
        // reads the registry for its name any more.
        for declared in crate::registry::read().declared() {
            let ext_dir = ext_skill_dir(declared.name())?;
            if skill::remove(&ext_dir)? {
                change.absorb(Change::changed(format!("removed {}", ext_dir.display())));
            }
        }
        change.absorb(mcp::uninstall(&mcp_spec()?)?);
        Ok(change)
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

    /// Plain stdout, the same as Claude Code — and nothing at all on a
    /// tool, because Codex documents no channel there. Naming one that is
    /// not there is worse than saying nothing.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool {
            return None;
        }
        (!reply.context.is_empty()).then(|| reply.joined())
    }

    fn has_skill(&self) -> bool {
        skill_dir().is_ok_and(|dir| skill::installed(&dir))
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
    fn the_briefing_goes_out_as_plain_text_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::ContextStart);
        reply.context.push("hello".into());
        assert_eq!(Codex.reply_envelope(&reply).as_deref(), Some("hello"));

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Codex.reply_envelope(&reply).is_none());
    }
}
