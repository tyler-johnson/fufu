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
//! The migration from settings entries to the plugin is add-then-remove:
//! install the plugin, verify it, then strip the settings entries. The
//! other order leaves a window with no capture at all; this one leaves a
//! window with double capture, which is safe — the snapshot is idempotent,
//! and only the briefing marker would race.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, Correction, EventKind, InstallOptions, Integration,
    Mechanism, Presence, Status, Wiring, payload, settings, skill,
};

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

const EVENTS: [(&str, Option<&str>); 2] = [
    ("PreToolUse", Some("Bash|Edit|Write|NotebookEdit")),
    // Claude stays on UserPromptSubmit rather than SessionStart. It has
    // both, and moving would delete the marker dance, but this path is
    // proven and running; the per-slug marker makes a per-turn channel and
    // a per-session one behave the same anyway.
    ("UserPromptSubmit", None),
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
    let hooks = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": EVENTS[0].1,
                "hooks": [{ "type": "command", "command": command }],
            }],
            "UserPromptSubmit": [{
                "hooks": [{ "type": "command", "command": command }],
            }],
        }
    });
    (pretty(&manifest), pretty(&hooks))
}

fn pretty(value: &serde_json::Value) -> String {
    let mut body = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    body.push('\n');
    body
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
    let missing: Vec<&str> = EVENTS
        .iter()
        .map(|(event, _)| *event)
        .filter(|event| {
            !value["hooks"][*event].as_array().is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry["hooks"].as_array().is_some_and(|cmds| {
                        cmds.iter()
                            .any(|c| c["command"].as_str().is_some_and(is_ours))
                    })
                })
            })
        })
        .collect();
    let plugin = plugin_dir().unwrap_or_default();
    match missing.len() {
        0 => Wiring::Wired {
            mechanism: Mechanism::Plugin,
            at: plugin,
        },
        n if n == EVENTS.len() => Wiring::NotWired,
        _ => Wiring::Partial {
            missing: missing.join(", "),
            at: plugin,
        },
    }
}

fn write_plugin() -> Result<()> {
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
    Ok(())
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
        // Only a retired *command spelling* is stale; a skill that has
        // drifted is its own row, because it is its own repair.
        let stale = spec().map(|spec| settings::stale(&spec)).unwrap_or(false);
        Status {
            slug: self.slug(),
            presence: self.detect(),
            wiring,
            note,
            parts: Vec::new(),
            skill: Some(skill_wiring()),
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

        write_plugin()?;
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

    /// Claude Code reads a hook's stdout as context, verbatim.
    fn briefing_envelope(&self, text: &str) -> String {
        text.to_string()
    }

    /// `PreToolUse` is the one correction channel that is documented and
    /// testable, which is why Claude Code is the only adapter that
    /// implements this.
    ///
    /// A denial carries `permissionDecision`. A coach carries
    /// `additionalContext` and **no `permissionDecision` at all** — that
    /// omission is load-bearing, because emitting `"allow"` would suppress
    /// the user's own permission prompts for a tool fufu was only
    /// commenting on. Plain stdout is not an option here: on `PreToolUse`
    /// it goes to the debug log, unlike `UserPromptSubmit` where the
    /// briefing lives.
    fn correction_envelope(&self, correction: &Correction) -> Option<String> {
        let mut hook = serde_json::Map::new();
        hook.insert("hookEventName".into(), "PreToolUse".into());
        if correction.deny {
            hook.insert("permissionDecision".into(), "deny".into());
            hook.insert(
                "permissionDecisionReason".into(),
                correction.text.clone().into(),
            );
        } else {
            hook.insert("additionalContext".into(), correction.text.clone().into());
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
        assert_eq!(Claude.briefing_envelope("hello"), "hello");
    }

    /// A denial says deny and carries a reason; a coach says nothing about
    /// permission at all, because `"allow"` would suppress the user's own
    /// prompt for a tool fufu was only commenting on.
    #[test]
    fn a_correction_speaks_pretooluse() {
        let deny = Claude
            .correction_envelope(&Correction {
                text: "run ff commit".into(),
                deny: true,
            })
            .expect("claude has a channel");
        let value: serde_json::Value = serde_json::from_str(&deny).unwrap();
        let hook = &value["hookSpecificOutput"];
        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "deny");
        assert_eq!(hook["permissionDecisionReason"], "run ff commit");

        let coach = Claude
            .correction_envelope(&Correction {
                text: "run ff commit".into(),
                deny: false,
            })
            .expect("claude has a channel");
        let value: serde_json::Value = serde_json::from_str(&coach).unwrap();
        let hook = &value["hookSpecificOutput"];
        assert_eq!(hook["additionalContext"], "run ff commit");
        assert!(
            hook.get("permissionDecision").is_none(),
            "a coach must not decide permission: {coach}"
        );
    }
}
