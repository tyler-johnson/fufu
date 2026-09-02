//! One JSON merge engine for every client whose wiring is a settings file
//! fufu does not own.
//!
//! Because the file belongs to the user, install parses it, adds exactly
//! fufu's entries, and writes back everything else untouched; uninstall
//! removes exactly what install added; a file that is not valid JSON, or
//! whose shape is not what the schema says, is refused with the file
//! untouched rather than rewritten into something the client cannot read.
//!
//! Two shapes cover the four clients. `Shape::Nested` is the one Claude
//! Code, Codex, and Gemini CLI share — an event maps to entries, and an
//! entry holds a matcher and a list of commands. `Shape::Flat` is Cursor's
//! — an entry *is* a command.

use std::path::{Path, PathBuf};

use ff_core::{Error, Result};
use serde_json::{Map, Value};

use super::{Change, Wiring};

/// Whether a missing event is an outage or an upgrade.
///
/// The distinction exists because "some of the events are wired" stopped
/// being one thing. An install predating an event fufu has since added has
/// whole capture and is simply old; an install missing the event capture
/// *depends* on is half-finished. Calling both `Partial` tells the first
/// user their capture is broken when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Capture depends on it. Missing means [`Wiring::Partial`].
    Required,
    /// It widens capture rather than founding it. Missing means `stale`,
    /// which routes the user to `ff doctor --fix` without claiming an
    /// outage.
    Extra,
}

/// How a client spells one hook entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `{"matcher": …, "hooks": [{"type": "command", "command": …}]}`
    Nested,
    /// `{"matcher": …, "command": …}`
    Flat,
}

/// One client's settings file, and what fufu writes into it.
pub struct Spec {
    pub path: PathBuf,
    pub shape: Shape,
    /// The events fufu hooks: the name, the matcher it takes (if any), and
    /// whether capture depends on it.
    pub events: &'static [(&'static str, Option<&'static str>, Need)],
    /// The command the client is told to run.
    pub command: String,
    /// Spellings older installs may still carry. Recognized as ours — so
    /// uninstall removes them and install upgrades rather than duplicates —
    /// because a settings file is only rewritten when someone runs the
    /// installer again, which they may never do.
    pub legacy: &'static [&'static str],
    /// A schema version the file must carry at its top level, when the
    /// client requires one (Cursor's `"version": 1`).
    pub version: Option<u64>,
}

impl Spec {
    fn is_ours(&self, command: Option<&str>) -> bool {
        command.is_some_and(|cmd| cmd == self.command || self.legacy.contains(&cmd))
    }

    /// Whether one entry runs fufu's command.
    fn entry_is_ours(&self, entry: &Value) -> bool {
        match self.shape {
            Shape::Nested => entry["hooks"]
                .as_array()
                .is_some_and(|cmds| cmds.iter().any(|c| self.is_ours(c["command"].as_str()))),
            Shape::Flat => self.is_ours(entry["command"].as_str()),
        }
    }

    /// The entry install appends for one event.
    fn entry_for(&self, matcher: Option<&str>) -> Value {
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        match self.shape {
            Shape::Nested => {
                entry.insert(
                    "hooks".into(),
                    serde_json::json!([{ "type": "command", "command": self.command }]),
                );
            }
            Shape::Flat => {
                entry.insert("command".into(), self.command.as_str().into());
            }
        }
        Value::Object(entry)
    }

    /// Rewrite a legacy spelling in place. Returns whether anything moved.
    fn upgrade(&self, entry: &mut Value) -> bool {
        let mut changed = false;
        let mut upgrade_one = |slot: &mut Value| {
            if slot["command"]
                .as_str()
                .is_some_and(|c| self.legacy.contains(&c))
            {
                slot["command"] = self.command.as_str().into();
                changed = true;
            }
        };
        match self.shape {
            Shape::Nested => {
                if let Some(cmds) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                    for cmd in cmds.iter_mut() {
                        upgrade_one(cmd);
                    }
                }
            }
            Shape::Flat => upgrade_one(entry),
        }
        changed
    }
}

/// The file as an object, with a missing file reading as an empty one.
/// Shared with the MCP registration, which merges one key into the same
/// files on the same rules.
pub(super) fn load(path: &Path) -> Result<Map<String, Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::from("{}"),
        Err(err) => return Err(Error::repo(err)),
    };
    let value: Value = serde_json::from_str(&text).map_err(|err| {
        Error::msg(format!(
            "{}: not valid JSON ({err}); file untouched",
            path.display()
        ))
    })?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Error::msg(format!(
            "{}: top level is not an object; file untouched",
            path.display()
        ))),
    }
}

pub(super) fn write(path: &Path, settings: &Map<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    let mut body =
        serde_json::to_string_pretty(&Value::Object(settings.clone())).map_err(Error::repo)?;
    body.push('\n');
    let tmp = path.with_extension("ff-tmp");
    std::fs::write(&tmp, &body).map_err(Error::repo)?;
    std::fs::rename(&tmp, path).map_err(Error::repo)?;
    Ok(())
}

/// Which of the spec's events are wired, in the spec's own order.
fn wired_events(spec: &Spec, settings: &Map<String, Value>) -> Vec<bool> {
    let hooks = settings.get("hooks").and_then(Value::as_object);
    spec.events
        .iter()
        .map(|(event, ..)| {
            hooks
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .is_some_and(|entries| entries.iter().any(|e| spec.entry_is_ours(e)))
        })
        .collect()
}

/// The one wiring answer `ff hook -l` and `ff doctor` both read.
pub fn wiring(spec: &Spec) -> Wiring {
    let settings = match load(&spec.path) {
        Ok(settings) => settings,
        Err(err) => return Wiring::Unavailable(err.to_string()),
    };
    let wired = wired_events(spec, &settings);
    if wired.iter().all(|w| !*w) {
        return Wiring::NotWired;
    }
    // Only a required event's absence is partial capture. An extra one is
    // missing from an install written before fufu grew it, which is stale
    // rather than broken.
    let missing = missing_of(spec, &wired, Need::Required);
    if missing.is_empty() {
        return Wiring::Wired {
            mechanism: super::Mechanism::Settings,
            at: spec.path.clone(),
        };
    }
    Wiring::Partial {
        missing: missing.join(", "),
        at: spec.path.clone(),
    }
}

/// The events of one need that are not wired, in the spec's own order.
fn missing_of(spec: &Spec, wired: &[bool], need: Need) -> Vec<&'static str> {
    spec.events
        .iter()
        .zip(wired)
        .filter(|((_, _, this), wired)| *this == need && !**wired)
        .map(|((event, ..), _)| *event)
        .collect()
}

/// Whether the wiring works but is written the way fufu no longer writes
/// it — a retired command spelling, or an install predating an event fufu
/// has since added. Neither costs capture, so this is a repair to offer
/// and never an outage to report.
pub fn stale(spec: &Spec) -> bool {
    let Ok(settings) = load(&spec.path) else {
        return false;
    };
    let wired = wired_events(spec, &settings);
    if wired.iter().any(|w| *w) && !missing_of(spec, &wired, Need::Extra).is_empty() {
        return true;
    }
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    spec.events.iter().any(|(event, ..)| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .is_some_and(|entries| entries.iter().any(|entry| entry_is_legacy(spec, entry)))
    })
}

fn entry_is_legacy(spec: &Spec, entry: &Value) -> bool {
    let legacy = |slot: &Value| {
        slot["command"]
            .as_str()
            .is_some_and(|c| spec.legacy.contains(&c))
    };
    match spec.shape {
        Shape::Nested => entry["hooks"]
            .as_array()
            .is_some_and(|cmds| cmds.iter().any(legacy)),
        Shape::Flat => legacy(entry),
    }
}

pub fn install(spec: &Spec) -> Result<Change> {
    let mut settings = load(&spec.path)?;
    let mut changed = false;

    if let Some(version) = spec.version
        && settings.get("version").and_then(Value::as_u64) != Some(version)
    {
        settings.insert("version".into(), version.into());
        changed = true;
    }

    let hooks = settings
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut().ok_or_else(|| {
        Error::msg(format!(
            "{}: \"hooks\" is not an object; file untouched",
            spec.path.display()
        ))
    })?;

    for (event, matcher, _) in spec.events {
        let entries = hooks
            .entry((*event).to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let entries = entries.as_array_mut().ok_or_else(|| {
            Error::msg(format!(
                "{}: hooks.{event} is not an array; file untouched",
                spec.path.display()
            ))
        })?;
        // Upgrade any legacy spelling before the idempotence check, so an
        // old entry is rewritten rather than joined by a second one.
        for entry in entries.iter_mut() {
            changed |= spec.upgrade(entry);
        }
        if entries.iter().any(|e| spec.entry_is_ours(e)) {
            continue;
        }
        entries.push(spec.entry_for(*matcher));
        changed = true;
    }

    if changed {
        write(&spec.path, &settings)?;
    }
    Ok(Change {
        changed,
        lines: vec![if changed {
            format!("wired into {}", spec.path.display())
        } else {
            format!("already wired in {}", spec.path.display())
        }],
    })
}

pub fn uninstall(spec: &Spec) -> Result<Change> {
    let mut settings = load(&spec.path)?;
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(Change::unchanged(format!(
            "nothing wired in {}",
            spec.path.display()
        )));
    };

    let mut changed = false;
    for (event, ..) in spec.events {
        let Some(entries) = hooks.get_mut(*event).and_then(Value::as_array_mut) else {
            continue;
        };
        match spec.shape {
            // An entry can hold several commands, only one of which is
            // ours: empty it, then drop it if that left nothing.
            Shape::Nested => {
                for entry in entries.iter_mut() {
                    let Some(cmds) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
                        continue;
                    };
                    let before = cmds.len();
                    cmds.retain(|c| !spec.is_ours(c["command"].as_str()));
                    changed |= cmds.len() != before;
                }
                let before = entries.len();
                // Foreign entries keep whatever shape they had.
                entries.retain(|entry| {
                    entry["hooks"]
                        .as_array()
                        .is_none_or(|cmds| !cmds.is_empty())
                });
                changed |= entries.len() != before;
            }
            Shape::Flat => {
                let before = entries.len();
                entries.retain(|entry| !spec.entry_is_ours(entry));
                changed |= entries.len() != before;
            }
        }
        if entries.is_empty() {
            hooks.remove(*event);
            changed = true;
        }
    }
    if hooks.is_empty() {
        settings.remove("hooks");
        changed = true;
    }

    if changed {
        write(&spec.path, &settings)?;
        Ok(Change::changed(format!(
            "removed from {}",
            spec.path.display()
        )))
    } else {
        Ok(Change::unchanged(format!(
            "nothing wired in {}",
            spec.path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(dir: &Path, shape: Shape) -> Spec {
        Spec {
            path: dir.join("settings.json"),
            shape,
            events: &[
                ("PreToolUse", Some("Bash|Edit"), Need::Required),
                ("SessionStart", None, Need::Required),
            ],
            command: "ff trigger test".into(),
            legacy: &["ff hook agent trigger test"],
            version: None,
        }
    }

    #[test]
    fn install_is_idempotent_and_uninstall_is_exact() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = spec(tmp.path(), Shape::Nested);
        std::fs::write(
            &spec.path,
            r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();

        assert!(install(&spec).unwrap().changed);
        assert!(!install(&spec).unwrap().changed);
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));

        assert!(uninstall(&spec).unwrap().changed);
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&spec.path).unwrap()).unwrap();
        assert_eq!(after["model"], "opus");
        // The foreign entry survives, alone.
        assert_eq!(after["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            after["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "other"
        );
        assert!(after["hooks"].get("SessionStart").is_none());
    }

    #[test]
    fn a_legacy_spelling_is_upgraded_rather_than_duplicated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = spec(tmp.path(), Shape::Nested);
        std::fs::write(
            &spec.path,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash|Edit","hooks":[{"type":"command","command":"ff hook agent trigger test"}]}]}}"#,
        )
        .unwrap();
        // A legacy entry already counts as wired, so capture never stops —
        // and it reads as stale, which is what `ff doctor --fix` repairs.
        assert!(matches!(wiring(&spec), Wiring::Partial { .. }));
        assert!(stale(&spec));
        assert!(install(&spec).unwrap().changed);
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&spec.path).unwrap()).unwrap();
        let entries = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "upgraded in place: {entries:?}");
        assert_eq!(entries[0]["hooks"][0]["command"], "ff trigger test");
        assert!(!stale(&spec), "the repair is what clears it");
    }

    #[test]
    fn the_flat_shape_writes_and_removes_a_bare_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = spec(tmp.path(), Shape::Flat);
        spec.version = Some(1);
        assert!(install(&spec).unwrap().changed);
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&spec.path).unwrap()).unwrap();
        assert_eq!(after["version"], 1);
        assert_eq!(
            after["hooks"]["PreToolUse"][0]["command"],
            "ff trigger test"
        );
        assert_eq!(after["hooks"]["PreToolUse"][0]["matcher"], "Bash|Edit");
        assert!(after["hooks"]["SessionStart"][0].get("matcher").is_none());

        assert!(uninstall(&spec).unwrap().changed);
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&spec.path).unwrap()).unwrap();
        assert!(after.get("hooks").is_none());
    }

    #[test]
    fn a_malformed_file_is_refused_untouched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = spec(tmp.path(), Shape::Nested);
        std::fs::write(&spec.path, "{ not json").unwrap();
        assert!(install(&spec).is_err());
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "{ not json");
        assert!(matches!(wiring(&spec), Wiring::Unavailable(_)));
    }
}
