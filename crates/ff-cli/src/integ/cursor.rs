//! Cursor's agent client.
//!
//! `cursor` is the agent, not the editor: a future editor integration gets
//! a slug of its own, because these slugs end up inside config files and
//! cannot be renamed afterward.
//!
//! Cursor CLI 2026.09.10 discovers user-local plugins under
//! `~/.cursor/plugins/local/<name>/` on a new session, with no marketplace
//! registration and no install command. fufu owns
//! `~/.cursor/plugins/local/fufu/` whole: the native
//! `.cursor-plugin/plugin.json` manifest, `hooks/hooks.json` in Cursor's
//! flat shape — an entry *is* a command — and the skill under
//! `skills/fufu/`. Plugin-only sessions fire `sessionStart`, `preToolUse`,
//! and `sessionEnd`; that build gates `beforeSubmitPrompt` and `stop` on
//! user or project settings even when plugin hooks exist, so those are not
//! wired. A resumed chat keeps its context and skips `sessionStart`. The
//! probe and its captures are in `tests/fixtures/cursor/`.
//!
//! Hooks run from the plugin directory and name the workspace in their
//! payload: `sessionStart` and `sessionEnd` carry no `cwd`, and a shell
//! `preToolUse` carries an empty one, so the first non-empty
//! `workspace_roots` entry is the directory to discover from. The session
//! id moves too: tool events carry `conversation_id` while `sessionStart`
//! carries `session_id`. Both are read, so a session is one id either way
//! and the briefing marker does not thrash. Only the shell gets
//! `CURSOR_AGENT` and `CURSOR_CONVERSATION_ID`.
//!
//! The settings file an earlier fufu merged into, `~/.cursor/hooks.json`,
//! still reads as wired so `ff hook -u` migrates it: the install writes
//! the plugin, verifies it, then strips its own entries there and the MCP
//! registration a fufu before v0.15 wrote into `~/.cursor/mcp.json`. Cloud
//! agents get no user-local hooks, so nothing captures there.

use std::path::PathBuf;

use ff_core::Result;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::settings::{Event, Need};
use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Mechanism, Presence,
    Reply, Status, Wiring, mcp, payload, plugin, settings, skill,
};

pub struct Cursor;

const NAME: &str = "fufu";
const TAIL: &str = "trigger cursor";

const NOTE: &str = "Cursor loads local plugins in trusted workspaces, and team policy must allow local \
                    plugin imports; cloud agents get no user-local hooks, so nothing captures there";

/// The three events a plugin-only session fires. `preToolUse` is the one
/// capture cannot miss; `sessionStart` is where the briefing lands, and
/// since there is no prompt event it is required too; `sessionEnd` widens
/// capture.
const EVENTS: [Event; 3] = [
    ("preToolUse", Some("Shell|Write|Delete"), Need::Required),
    ("sessionStart", None, Need::Required),
    ("sessionEnd", None, Need::Extra),
];

/// What the settings-merge adapter wrote into `~/.cursor/hooks.json`,
/// kept so the migration can strip it and status can still see it.
const OLD_EVENTS: [Event; 2] = [EVENTS[0], EVENTS[1]];
const OLD_COMMAND: &str = "ff trigger cursor";
const OLD_LEGACY: [&str; 0] = [];

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".cursor"))
}

fn plugin_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("plugins").join("local").join(NAME))
}

fn manifest_path(dir: &std::path::Path) -> PathBuf {
    dir.join(".cursor-plugin").join("plugin.json")
}

fn hooks_path(dir: &std::path::Path) -> PathBuf {
    dir.join("hooks").join("hooks.json")
}

fn skill_dir(dir: &std::path::Path) -> PathBuf {
    dir.join("skills").join(skill::NAME)
}

/// `cursor-agent` is the installer's unambiguous alias; `agent` is its
/// primary name. `FF_CURSOR` is the seam that isolates tests from the
/// developer's client.
fn binary() -> Option<PathBuf> {
    super::client_binary(
        "FF_CURSOR",
        &[
            "cursor-agent",
            "cursor-agent.exe",
            "cursor-agent.cmd",
            "agent",
            "agent.exe",
            "agent.cmd",
        ],
    )
}

fn old_spec() -> Result<settings::Spec> {
    Ok(settings::Spec {
        path: config_dir()?.join("hooks.json"),
        shape: settings::Shape::Flat,
        events: &OLD_EVENTS,
        command: OLD_COMMAND.into(),
        legacy: &OLD_LEGACY,
        version: Some(1),
    })
}

/// Where a fufu before v0.15 registered its MCP server: `mcp.json`, which
/// is where Cursor reads servers from; the hooks file is not it.
fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec {
        path: config_dir()?.join("mcp.json"),
        shape: mcp::Shape::Json,
    })
}

// ---- the plugin ------------------------------------------------------------

fn bodies() -> (String, String) {
    let manifest = plugin::pretty(&serde_json::json!({
        "name": NAME, "version": env!("CARGO_PKG_VERSION"),
        "description": "fufu (ff) snapshots the working copy before every tool action",
        "homepage": env!("CARGO_PKG_REPOSITORY")
    }));
    let command = super::exe_command(TAIL);
    let mut hooks = Map::new();
    for (event, matcher, _) in EVENTS {
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert("command".into(), command.as_str().into());
        hooks.insert(event.into(), Value::Array(vec![Value::Object(entry)]));
    }
    (
        manifest,
        plugin::pretty(&serde_json::json!({ "version": 1, "hooks": hooks })),
    )
}

fn has_event(hooks: &Value, event: &str) -> bool {
    hooks["version"] == 1
        && hooks["hooks"][event].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["command"]
                    .as_str()
                    .is_some_and(|cmd| cmd.ends_with(TAIL))
            })
        })
}

fn plugin_wiring() -> Result<Wiring> {
    let dir = plugin_dir()?;
    if !dir.exists() {
        return Ok(Wiring::NotWired);
    }
    let manifest = Value::Object(settings::load(&manifest_path(&dir))?);
    let hooks = Value::Object(settings::load(&hooks_path(&dir))?);
    let mut missing = Vec::new();
    if manifest["name"] != NAME || !manifest["version"].is_string() {
        missing.push("Cursor manifest");
    }
    for (event, _, need) in &EVENTS {
        if *need == Need::Required && !has_event(&hooks, event) {
            missing.push(event);
        }
    }
    Ok(if missing.is_empty() {
        Wiring::Wired {
            mechanism: Mechanism::Plugin,
            at: dir,
        }
    } else {
        Wiring::Partial {
            missing: missing.join(", "),
            at: dir,
        }
    })
}

fn plugin_stale() -> bool {
    plugin_dir().is_ok_and(|dir| {
        settings::load(&hooks_path(&dir)).is_ok_and(|hooks| {
            let hooks = Value::Object(hooks);
            EVENTS.iter().any(|(e, ..)| has_event(&hooks, e))
                && EVENTS
                    .iter()
                    .any(|(e, _, need)| *need == Need::Extra && !has_event(&hooks, e))
        })
    })
}

// ---- the migration ---------------------------------------------------------

/// The verified plugin is already in place before this runs, so each strip
/// is best-effort: a malformed old file stays as found, with the failure
/// reported.
fn strip_old(change: &mut Change) {
    match remove_old_settings() {
        Ok(stripped) if stripped.changed => change.absorb(stripped),
        Ok(_) => {}
        Err(err) => change
            .lines
            .push(format!("left ~/.cursor/hooks.json as found: {err}")),
    }
    match mcp_spec().and_then(|spec| mcp::strip(&spec)) {
        Ok(stripped) => change.absorb(stripped),
        Err(err) => change
            .lines
            .push(format!("left ~/.cursor/mcp.json as found: {err}")),
    }
}

fn remove_old_settings() -> Result<Change> {
    let spec = old_spec()?;
    let settings = settings::load(&spec.path)?;
    if let Some(hooks) = settings.get("hooks") {
        let hooks = hooks
            .as_object()
            .ok_or_else(|| super::malformed(&spec.path, "\"hooks\" is not an object"))?;
        for (event, ..) in &OLD_EVENTS {
            if hooks.get(*event).is_some_and(|entries| !entries.is_array()) {
                return Err(super::malformed(
                    &spec.path,
                    format!("\"{event}\" is not an array"),
                ));
            }
        }
    }
    if !settings::wiring(&spec).feeds_capture() {
        return Ok(Change::unchanged("no old Cursor settings wiring"));
    }
    settings::uninstall(&spec)
}

// ---- the payload -----------------------------------------------------------

/// Cursor's payload. Same idea as the shared dialect, different names for
/// the one field that matters most, and the workspace where the cwd is
/// not.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Payload {
    hook_event_name: String,
    /// Tool events carry this one.
    conversation_id: String,
    /// `sessionStart` carries this one.
    session_id: String,
    cwd: String,
    /// The hook runs from the plugin directory and names the repository
    /// here, often without a cwd.
    workspace_roots: Vec<String>,
    tool_name: String,
    tool_input: payload::ToolInput,
    prompt: String,
}

impl Payload {
    /// The directory the session is in: the payload's cwd, then its first
    /// workspace root, then the process's own directory. The hook's cwd is
    /// the plugin directory, so the workspace fallback comes first.
    fn cwd(&self) -> Result<PathBuf> {
        if !self.cwd.is_empty() {
            return Ok(PathBuf::from(&self.cwd));
        }
        if let Some(root) = self.workspace_roots.iter().find(|root| !root.is_empty()) {
            return Ok(PathBuf::from(root));
        }
        std::env::current_dir().map_err(ff_core::Error::repo)
    }
}

// ---- the integration -------------------------------------------------------

impl Integration for Cursor {
    fn slug(&self) -> &'static str {
        "cursor"
    }

    fn detect(&self) -> Presence {
        if let Ok(dir) = config_dir()
            && dir.is_dir()
        {
            return Presence::Present { evidence: dir };
        }
        binary().map_or(Presence::Absent, |evidence| Presence::Present { evidence })
    }

    fn status(&self) -> Status {
        let old = old_spec()
            .map(|spec| settings::wiring(&spec))
            .unwrap_or(Wiring::NotWired);
        let wiring = match plugin_wiring() {
            // Old settings must remain visible to `ff hook -u`, so an
            // upgrade migrates them.
            Ok(Wiring::NotWired) => old.clone(),
            Ok(wiring) => wiring,
            Err(err) => Wiring::Unavailable {
                complaint: err.to_string(),
            },
        };
        let skill = plugin_dir().map_or(Wiring::NotWired, |dir| skill::wiring(&skill_dir(&dir)));
        let stale = old.feeds_capture() || plugin_stale();
        Status {
            note: wiring.feeds_capture().then(|| NOTE.into()),
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            parts: Vec::new(),
            skill: Some(skill),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let dir = plugin_dir()?;
        let (manifest, hooks) = bodies();
        let mut changed = plugin::write_if_changed(&manifest_path(&dir), &manifest)?;
        changed |= plugin::write_if_changed(&hooks_path(&dir), &hooks)?;
        let skills = skill_dir(&dir);
        if !matches!(skill::wiring(&skills), Wiring::Wired { .. }) {
            skill::write(&skills)?;
            changed = true;
        }
        let verified = plugin_wiring()?;
        if !matches!(verified, Wiring::Wired { .. }) {
            return Err(super::failed(
                &dir,
                format!(
                    "the plugin did not verify after the write ({}); the old wiring is left in place",
                    verified.word()
                ),
            ));
        }
        let line = format!(
            "{} in {}; Cursor discovers it on the next session",
            if changed {
                "plugin and skill written"
            } else {
                "already wired"
            },
            dir.display()
        );
        let mut change = if changed {
            Change::changed(line)
        } else {
            Change::unchanged(line)
        };
        strip_old(&mut change);
        change.lines.push(NOTE.into());
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let dir = plugin_dir()?;
        let mut change = if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|err| super::failed(&dir, err))?;
            Change::changed(format!("removed {}", dir.display()))
        } else {
            Change::unchanged("no fufu plugin installed")
        };
        strip_old(&mut change);
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
            raw.session_id.clone()
        } else {
            raw.conversation_id.clone()
        };
        let cwd = raw.cwd()?;
        let kind = forced
            .or_else(|| EventKind::from_hint(&raw.hook_event_name))
            .unwrap_or(EventKind::Other);
        let label = match kind {
            EventKind::BeforeTool if !raw.tool_name.is_empty() => {
                payload::tool_label(&raw.tool_name, &raw.tool_input)
            }
            _ => payload::event_label(kind, &raw.hook_event_name, &raw.prompt),
        };
        Ok(Some(AgentEvent {
            kind,
            session,
            // Cursor's payload names no subagent, so every event is the
            // main thread's.
            agent: String::new(),
            cwd,
            label,
            command: payload::command_of(&raw.tool_input),
            tool: payload::tool_of(kind, &raw.tool_name),
            path: payload::path_of(kind, &raw.tool_input),
        }))
    }

    /// Cursor takes injected context as a JSON field, the way Qwen does,
    /// under its own name — and documents no channel on a tool, so nothing
    /// is said there.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind == EventKind::BeforeTool || reply.context.is_empty() {
            return None;
        }
        Some(serde_json::json!({ "additional_context": reply.joined() }).to_string())
    }

    fn has_skill(&self) -> bool {
        plugin_dir().is_ok_and(|dir| skill::installed(&skill_dir(&dir)))
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

    /// The hook runs from the plugin directory: an empty or missing cwd
    /// falls back to the first workspace root, and an explicit cwd wins.
    #[test]
    fn the_workspace_root_stands_in_for_a_missing_cwd() {
        let event = Cursor
            .parse(
                br#"{"hook_event_name":"preToolUse","conversation_id":"c","cwd":"",
                     "workspace_roots":["","/ws"],"tool_name":"Shell","tool_input":{"command":"ls"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.cwd, std::path::Path::new("/ws"));
        let event = Cursor
            .parse(
                br#"{"hook_event_name":"sessionStart","session_id":"c","cwd":"/here",
                     "workspace_roots":["/ws"]}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.cwd, std::path::Path::new("/here"));
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

    /// The native manifest and the flat hooks file: the matcher on
    /// `preToolUse` alone, every entry a bare command, and each reading
    /// back by its event.
    #[test]
    fn the_bodies_are_the_native_manifest_and_flat_hooks() {
        let (manifest, hooks) = bodies();
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["name"], "fufu");
        assert!(manifest.get("$schema").is_none());
        let hooks: Value = serde_json::from_str(&hooks).unwrap();
        assert_eq!(hooks["version"], 1);
        assert_eq!(hooks["hooks"].as_object().unwrap().len(), EVENTS.len());
        assert_eq!(
            hooks["hooks"]["preToolUse"][0]["matcher"],
            "Shell|Write|Delete"
        );
        assert!(hooks["hooks"]["sessionStart"][0].get("matcher").is_none());
        for (event, ..) in EVENTS {
            assert!(has_event(&hooks, event), "{event}: {hooks}");
            assert!(hooks["hooks"][event][0].get("hooks").is_none());
        }
        assert!(!has_event(&hooks, "beforeSubmitPrompt"));
    }
}
