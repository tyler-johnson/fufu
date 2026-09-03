//! Claude Code.
//!
//! The one client fufu can wire through a directory it owns outright. A
//! plugin under `~/.claude/skills/fufu/` auto-loads with no marketplace and
//! no install step, so install writes the directory whole and uninstall
//! removes it — none of the parse-preserve-foreign-entries machinery a
//! settings file needs, because there is no foreign content in a directory
//! that is entirely fufu's. `--settings` is the escape hatch back to
//! settings entries if the plugin path ever misbehaves.
//!
//! The plugin carries the shipped skill too, under `skills/fufu/`, which
//! is the layout a plugin's own skills take. That is why the skill costs
//! this adapter nothing structural: it is one more file inside a directory
//! that is written whole and removed whole either way. `--settings` gets
//! no skill, because the skill rides the plugin.
//!
//! A declared extension's own skill files land the same way, beside
//! `skills/fufu/` under `skills/<name>/` — one directory per extension,
//! nested inside the plugin fufu already owns, so writing and removing it
//! whole with the rest costs nothing structural either. `--settings` gets
//! none of them, on the same rule fufu's own skill does not.
//!
//! The MCP server rides the plugin too, as its `.mcp.json`: the fourth
//! file in the directory, written and removed with the other three. That
//! is why `--settings` carries no server, on the same rule as the skill.
//!
//! The migration from settings entries to the plugin is add-then-remove:
//! install the plugin, verify it, then strip the settings entries. The
//! other order leaves a window with no capture at all; this one leaves a
//! window with double capture, which is safe — the snapshot is idempotent,
//! and only the briefing marker would race.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Mechanism, Presence,
    Reply, Status, Wiring, mcp, payload, settings, skill,
};
use settings::Need;

pub struct Claude;

/// The canonical trigger command, and the spellings older installs carry.
/// A stored string is accepted forever: it sits in a file fufu can only
/// rewrite when somebody runs the installer again, which they may never do.
const COMMAND: &str = "ff trigger claude";
const LEGACY: [&str; 2] = ["ff hook agent trigger claude", "ff hook claude"];

/// The plugin bakes an absolute path, so recognizing our own wiring cannot
/// be an equality test: the binary moves, and a moved binary must still
/// read as wired rather than as gone.
const TAIL: &str = "trigger claude";

/// Every event fufu wires, and what each one is for.
///
/// The first two found the floor and are required. `PreToolUse` is the one
/// capture cannot miss, and the only channel that reaches a subagent or a
/// repository the agent has just entered. `UserPromptSubmit` is the turn
/// boundary the briefing has always ridden.
///
/// The rest are extra: they widen capture rather than found it, so an
/// install predating them is stale and not partial. `SessionStart` is where
/// the briefing is rebuilt after the context it was injected into was
/// dropped or truncated. `Stop` and `SubagentStop` make the last edit of a
/// turn durable — capture is snapshot-*before*, so without them the file
/// state an agent writes as its final action sits uncaptured until
/// whatever comes next, and a session that ends there never snapshots it.
/// `SubagentStart` lays a floor before a subagent writes anything.
///
/// `CwdChanged` is redundant with the `Bash` call that did the `cd`, and
/// earns its place anyway: that call captured against the *old*
/// repository, and this is the floor in the new one before anything writes
/// there. These are this vendor's spellings, so this list is Claude's
/// alone — the other three adapters keep their two events.
const EVENTS: [(&str, Option<&str>, Need); 7] = [
    (
        "PreToolUse",
        Some("Bash|Edit|Write|NotebookEdit"),
        Need::Required,
    ),
    ("UserPromptSubmit", None, Need::Required),
    (
        "SessionStart",
        Some("startup|resume|clear|compact|fork"),
        Need::Extra,
    ),
    ("Stop", None, Need::Extra),
    ("SubagentStop", None, Need::Extra),
    ("SubagentStart", None, Need::Extra),
    ("CwdChanged", None, Need::Extra),
];

fn config_dir() -> Result<PathBuf> {
    Ok(super::home()?.join(".claude"))
}

fn plugin_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("skills/fufu"))
}

fn manifest_path() -> Result<PathBuf> {
    Ok(plugin_dir()?.join(".claude-plugin/plugin.json"))
}

fn hooks_path() -> Result<PathBuf> {
    Ok(plugin_dir()?.join("hooks/hooks.json"))
}

/// Where a plugin's own skills live, which is where Claude Code looks for
/// them — the plugin directory is not itself the skill.
fn skill_dir() -> Result<PathBuf> {
    Ok(plugin_dir()?.join("skills").join(skill::NAME))
}

fn skill_wiring() -> Wiring {
    match skill_dir() {
        Ok(dir) => skill::wiring(&dir),
        Err(_) => Wiring::NotWired,
    }
}

/// The plugin's `.mcp.json`, which is the one MCP file a plugin carries.
fn mcp_spec() -> Result<mcp::Spec> {
    Ok(mcp::Spec::new(
        plugin_dir()?.join(".mcp.json"),
        mcp::Shape::Json { with_type: true },
    ))
}

fn mcp_wiring() -> Wiring {
    match mcp_spec() {
        Ok(spec) => mcp::wiring(&spec),
        Err(_) => Wiring::NotWired,
    }
}

/// Every declared extension's own server in the plugin's `.mcp.json`, and
/// every name registered there that nothing declares any more.
fn mcp_ext_status() -> (Vec<mcp::McpExtension>, Vec<String>) {
    match mcp_spec() {
        Ok(spec) => mcp::extensions(&spec),
        Err(_) => (Vec::new(), Vec::new()),
    }
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

fn is_ours(command: &str) -> bool {
    command.ends_with(TAIL) || LEGACY.contains(&command)
}

// ---- the plugin ------------------------------------------------------------

fn plugin_body() -> (String, String) {
    let command = super::exe_command("trigger claude");
    let manifest = serde_json::json!({
        "name": "fufu",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "fufu (ff) snapshots the working tree before every tool action",
        "homepage": env!("CARGO_PKG_REPOSITORY"),
    });
    let mut events = serde_json::Map::new();
    for (event, matcher, _) in EVENTS {
        let mut entry = serde_json::Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert(
            "hooks".into(),
            serde_json::json!([{ "type": "command", "command": command }]),
        );
        events.insert(
            event.to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(entry)]),
        );
    }
    let hooks = serde_json::json!({ "hooks": serde_json::Value::Object(events) });
    (pretty(&manifest), pretty(&hooks))
}

fn pretty(value: &serde_json::Value) -> String {
    let mut body = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    body.push('\n');
    body
}

/// Which of `EVENTS` this hooks.json does not carry, in `EVENTS` order.
fn plugin_missing(value: &serde_json::Value) -> Vec<(&'static str, Need)> {
    EVENTS
        .iter()
        .filter(|(event, ..)| {
            !value["hooks"][*event].as_array().is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry["hooks"].as_array().is_some_and(|cmds| {
                        cmds.iter()
                            .any(|c| c["command"].as_str().is_some_and(is_ours))
                    })
                })
            })
        })
        .map(|(event, _, need)| (*event, *need))
        .collect()
}

/// The plugin's hooks.json, when there is one that parses.
fn plugin_hooks() -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(hooks_path().ok()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether the plugin on disk is wired, read the way the client reads it.
fn plugin_wiring() -> Wiring {
    let Ok(path) = hooks_path() else {
        return Wiring::NotWired;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Wiring::NotWired;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Wiring::Unavailable(format!("{}: not valid JSON", path.display()));
    };
    let missing = plugin_missing(&value);
    let plugin = plugin_dir().unwrap_or_default();
    if missing.len() == EVENTS.len() {
        return Wiring::NotWired;
    }
    // Only a required event's absence is partial capture; an extra one
    // missing is an older install, which `plugin_stale` reports instead.
    let required: Vec<&str> = missing
        .iter()
        .filter(|(_, need)| *need == Need::Required)
        .map(|(event, _)| *event)
        .collect();
    if required.is_empty() {
        return Wiring::Wired {
            mechanism: Mechanism::Plugin,
            at: plugin,
        };
    }
    Wiring::Partial {
        missing: required.join(", "),
        at: plugin,
    }
}

/// Whether the plugin is missing an event a current install would write.
/// Capture is whole without one — that is what `Need::Extra` means — so
/// this is a repair to offer and never an outage to report.
fn plugin_stale() -> bool {
    let Some(value) = plugin_hooks() else {
        return false;
    };
    let missing = plugin_missing(&value);
    missing.len() < EVENTS.len() && missing.iter().any(|(_, need)| *need == Need::Extra)
}

/// A declared extension's own skill directory, beside `skills/fufu` inside
/// the plugin — nested inside a directory fufu already owns outright, so
/// writing and removing it whole costs nothing structural either.
fn ext_skill_dir(name: &str) -> Result<PathBuf> {
    Ok(plugin_dir()?.join("skills").join(name))
}

/// Every declared extension's skill, written or refreshed beside fufu's own.
/// Answers the ones that actually landed a file, in registry order, so the
/// caller can report each and say nothing about one that named no readable
/// skill.
fn write_ext_skills() -> Result<Vec<String>> {
    let mut written = Vec::new();
    for declared in crate::registry::read().declared() {
        let dir = ext_skill_dir(declared.name())?;
        if skill::write_sources(&dir, declared)? > 0 {
            written.push(declared.name().to_string());
        }
    }
    Ok(written)
}

/// Writes the plugin whole, and answers the extensions whose own skill
/// landed along with the server registration's own report — which names a
/// declared extension's server as well as fufu's, and is therefore the
/// installer's word on it rather than a line this adapter can spell.
fn write_plugin() -> Result<(Vec<String>, Change)> {
    let (manifest, hooks) = plugin_body();
    let manifest_path = manifest_path()?;
    let hooks_path = hooks_path()?;
    for path in [&manifest_path, &hooks_path] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ff_core::Error::repo)?;
        }
    }
    std::fs::write(&manifest_path, manifest).map_err(ff_core::Error::repo)?;
    std::fs::write(&hooks_path, hooks).map_err(ff_core::Error::repo)?;
    skill::write(&skill_dir()?)?;
    let ext_skills = write_ext_skills()?;
    // The merge engine on a file only fufu writes: the result is the whole
    // file, and the engine is what keeps the entries' shape in one place.
    let servers = mcp::install(&mcp_spec()?)?;
    Ok((ext_skills, servers))
}

fn remove_plugin() -> Result<bool> {
    let dir = plugin_dir()?;
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).map_err(ff_core::Error::repo)?;
    Ok(true)
}

// ---- the integration -------------------------------------------------------

impl Integration for Claude {
    fn slug(&self) -> &'static str {
        "claude"
    }

    fn detect(&self) -> Presence {
        match config_dir() {
            Ok(dir) if dir.is_dir() => Presence::Present { evidence: dir },
            _ => Presence::Absent,
        }
    }

    fn status(&self) -> Status {
        // The plugin is the mechanism, so it is read first; settings
        // entries are reported when they are what is actually there, which
        // is the state every existing install is in.
        let wiring = match plugin_wiring() {
            Wiring::NotWired => match spec() {
                Ok(spec) => settings::wiring(&spec),
                Err(err) => Wiring::Unavailable(err.to_string()),
            },
            other => other,
        };
        // Settings entries still capture, so this is a repair to offer and
        // never an outage: the mechanism moved, and the wiring did not.
        let on_settings = matches!(
            &wiring,
            Wiring::Wired {
                mechanism: Mechanism::Settings,
                ..
            } | Wiring::Partial { .. }
        ) && !matches!(plugin_wiring(), Wiring::Wired { .. });
        let note = on_settings.then(|| {
            "`ff hook claude` moves it to the plugin, which loads on the client's next restart"
                .to_string()
        });
        // Being on the older mechanism is news, not a finding: it still
        // captures, and moving costs a client restart. That move stays
        // something a person asks for with `ff hook claude`, which says so.
        // Only a retired *command spelling*, or an install written before
        // fufu grew an event, is stale; a skill that has drifted is its
        // own row, because it is its own repair, and so is the server.
        let stale = plugin_stale() || spec().map(|spec| settings::stale(&spec)).unwrap_or(false);
        let (mcp_extensions, mcp_orphaned) = mcp_ext_status();
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note,
            parts: Vec::new(),
            skill: Some(skill_wiring()),
            mcp: Some(mcp_wiring()),
            mcp_extensions,
            mcp_orphaned,
            stale,
        }
    }

    fn install(&self, opts: &InstallOptions) -> Result<Change> {
        if opts.settings {
            let mut change = settings::install(&spec()?)?;
            if remove_plugin()? {
                change.absorb(Change::changed("removed the fufu plugin"));
            }
            return Ok(change);
        }

        let (ext_skills, servers) = write_plugin()?;
        // Verify before removing the other wiring: a plugin that did not
        // land must not take the settings entries down with it.
        let verified = plugin_wiring();
        if !matches!(verified, Wiring::Wired { .. }) {
            return Err(ff_core::Error::msg(format!(
                "the plugin at {} did not verify ({}); settings entries left in place",
                plugin_dir()?.display(),
                verified.word()
            )));
        }
        let mut change = Change::changed(format!("plugin written to {}", plugin_dir()?.display()));
        // Now, and only now, the old wiring goes.
        let stripped = settings::uninstall(&spec()?)?;
        if stripped.changed {
            change.absorb(Change::changed(
                "moved off the settings entries it used to use",
            ));
        }
        change
            .lines
            .push(format!("skill written to {}", skill_dir()?.display()));
        for name in &ext_skills {
            change.lines.push(format!(
                "{name} skill written to {}",
                ext_skill_dir(name)?.display()
            ));
        }
        change.lines.extend(servers.lines);
        change.lines.push(
            "restart Claude Code to load it (`claude plugin list` shows it as fufu@skills-dir)"
                .into(),
        );
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        // Remove both mechanisms: uninstall takes back exactly what install
        // ever added, and which of the two it added depends on when.
        let mut change = if remove_plugin()? {
            Change::changed(format!("removed {}", plugin_dir()?.display()))
        } else {
            Change::unchanged("no fufu plugin installed")
        };
        let stripped = settings::uninstall(&spec()?)?;
        if stripped.changed {
            change.absorb(stripped);
        }
        Ok(change)
    }

    /// Repair in place: whichever mechanism this machine is already on
    /// stays. `ff doctor --fix` must never silently move somebody onto the
    /// plugin, because a running Claude Code will not load it until it
    /// restarts — capture would go dark with nothing saying why.
    fn repair(&self) -> Result<Change> {
        let on_plugin = matches!(plugin_wiring(), Wiring::Wired { .. });
        self.install(&InstallOptions {
            settings: !on_plugin,
        })
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Claude)
    }
}

impl AgentProtocol for Claude {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// Claude Code reads a hook's stdout as context, verbatim — except on
    /// `PreToolUse`, where plain stdout goes to the debug log and the only
    /// channel is a `hookSpecificOutput` object. That is the one documented
    /// and testable correction channel across the four clients, which is
    /// why Claude Code is the only adapter that has one.
    ///
    /// The object is emitted once and carries whatever fell due: the
    /// briefing as `additionalContext`, a refusal as `permissionDecision`,
    /// or both. A coach carries `additionalContext` and **no
    /// `permissionDecision` at all** — that omission is load-bearing,
    /// because emitting `"allow"` would suppress the user's own permission
    /// prompts for a tool fufu was only commenting on. `deny: None` is
    /// exactly what that omission now means.
    fn reply_envelope(&self, reply: &Reply) -> Option<String> {
        if reply.kind != EventKind::BeforeTool {
            return (!reply.context.is_empty()).then(|| reply.joined());
        }
        let mut hook = serde_json::Map::new();
        hook.insert("hookEventName".into(), "PreToolUse".into());
        if !reply.context.is_empty() {
            hook.insert("additionalContext".into(), reply.joined().into());
        }
        if let Some(reason) = &reply.deny {
            hook.insert("permissionDecision".into(), "deny".into());
            hook.insert("permissionDecisionReason".into(), reason.clone().into());
        }
        Some(
            serde_json::json!({ "hookSpecificOutput": serde_json::Value::Object(hook) })
                .to_string(),
        )
    }

    fn has_skill(&self) -> bool {
        skill_dir().is_ok_and(|dir| skill::installed(&dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::Label;

    fn parse(json: &str) -> AgentEvent {
        Claude
            .parse(json.as_bytes(), None)
            .expect("parses")
            .expect("an event")
    }

    /// The recorded payload shape, as Claude Code sends it.
    #[test]
    fn a_pretooluse_payload_becomes_a_beforetool_event() {
        let event = parse(
            r#"{"hook_event_name":"PreToolUse","session_id":"0123456789abcdef",
                "cwd":"/repo","tool_name":"Bash",
                "tool_input":{"command":"cargo test"}}"#,
        );
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "0123456789abcdef");
        assert_eq!(event.cwd, std::path::Path::new("/repo"));
        assert_eq!(event.label, Label::text("Bash(cargo test)"));
    }

    #[test]
    fn a_userpromptsubmit_payload_becomes_a_contextstart_event() {
        let event = parse(
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","cwd":"/repo",
                "prompt":"fix the tests"}"#,
        );
        assert_eq!(event.kind, EventKind::ContextStart);
        assert_eq!(event.label, Label::text("prompt \"fix the tests\""));
    }

    #[test]
    fn an_edit_names_its_path() {
        let event = parse(
            r#"{"hook_event_name":"PreToolUse","session_id":"s","cwd":"/repo",
                "tool_name":"Edit","tool_input":{"file_path":"/repo/src/lib.rs"}}"#,
        );
        assert_eq!(
            event.label,
            Label::Path {
                tool: "Edit".into(),
                path: "/repo/src/lib.rs".into()
            }
        );
    }

    /// The event hint from a `<vendor>-<event>` name overrides the
    /// payload's own field.
    #[test]
    fn a_forced_event_overrides_the_payload() {
        let event = Claude
            .parse(
                br#"{"hook_event_name":"UserPromptSubmit","session_id":"s","cwd":"/repo"}"#,
                Some(EventKind::BeforeTool),
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
    }

    #[test]
    fn the_briefing_goes_out_as_plain_text() {
        let mut reply = Reply::new(EventKind::ContextStart);
        reply.context.push("hello".into());
        assert_eq!(Claude.reply_envelope(&reply).as_deref(), Some("hello"));
    }

    /// A denial says deny and carries a reason; a coach says nothing about
    /// permission at all, because `"allow"` would suppress the user's own
    /// prompt for a tool fufu was only commenting on.
    #[test]
    fn a_reply_on_a_tool_speaks_pretooluse() {
        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.deny = Some("run ff commit".into());
        let deny = Claude.reply_envelope(&reply).expect("claude has a channel");
        let value: serde_json::Value = serde_json::from_str(&deny).unwrap();
        let hook = &value["hookSpecificOutput"];
        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "deny");
        assert_eq!(hook["permissionDecisionReason"], "run ff commit");
        assert!(
            hook.get("additionalContext").is_none(),
            "nothing was injected: {deny}"
        );

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("run ff commit".into());
        let coach = Claude.reply_envelope(&reply).expect("claude has a channel");
        let value: serde_json::Value = serde_json::from_str(&coach).unwrap();
        let hook = &value["hookSpecificOutput"];
        assert_eq!(hook["additionalContext"], "run ff commit");
        assert!(
            hook.get("permissionDecision").is_none(),
            "a coach must not decide permission: {coach}"
        );
    }

    /// The briefing and a refusal can fall due on one `PreToolUse`, and
    /// Claude Code parses a hook's stdout as a *single* object — so they
    /// arrive as one, carrying both.
    #[test]
    fn a_briefing_and_a_refusal_ride_one_object() {
        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("fufu is capturing".into());
        reply.deny = Some("run ff commit".into());
        let out = Claude.reply_envelope(&reply).expect("claude has a channel");
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        let hook = &value["hookSpecificOutput"];
        assert_eq!(hook["additionalContext"], "fufu is capturing");
        assert_eq!(hook["permissionDecision"], "deny");
        assert_eq!(hook["permissionDecisionReason"], "run ff commit");
    }

    /// Every event fufu wires is in the plugin it writes, with its matcher.
    #[test]
    fn the_plugin_carries_every_event() {
        let (_manifest, hooks) = plugin_body();
        let value: serde_json::Value = serde_json::from_str(&hooks).unwrap();
        for (event, matcher, _) in EVENTS {
            let entry = &value["hooks"][event][0];
            assert!(
                entry["hooks"][0]["command"].as_str().is_some_and(is_ours),
                "{event} runs fufu: {value}"
            );
            assert_eq!(
                entry.get("matcher").and_then(serde_json::Value::as_str),
                matcher,
                "{event} matcher"
            );
        }
    }
}
