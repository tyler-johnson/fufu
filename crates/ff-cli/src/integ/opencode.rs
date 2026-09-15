//! OpenCode.
//!
//! OpenCode extends through a JavaScript plugin module, not a hooks file:
//! every `{plugin,plugins}/*.{js,ts}` under its config directory loads at
//! startup. fufu writes one plugin file it owns whole,
//! `<config>/opencode/plugins/fufu.js`, from an embedded body
//! (`opencode.js`) with this binary's absolute path baked in as a string
//! literal, and removes it whole. The skill lands in OpenCode's own skills
//! directory beside it. `opencode.json` is not touched: its `plugin` field
//! is for npm packages.
//!
//! The plugin does three things, on the three hooks it registers.
//! `tool.execute.before` is the floor: a `PreToolUse` payload carrying the
//! tool's name and its arguments — OpenCode's `bash` args carry `command`,
//! so the label is `bash(<cmd>)` and the gitPolicy tally counts it — and
//! nothing printed, since OpenCode discards the output; no coaching
//! reaches the model here. `experimental.chat.system.transform` runs on
//! every model call and pushes the trigger's stdout onto the system
//! prompt: its payload spells `SessionStart`, which re-briefs
//! unconditionally, so under OpenCode the briefing is standing rather
//! than once per boundary, and survives compaction without a second
//! event; the capture it takes on an unchanged tree adds nothing.
//! `shell.env` sets `OPENCODE_SESSION_ID` on every shell command — the
//! bash tool's and the TUI's `!` shell — so a shell verb under OpenCode
//! carries the session its hook captures do.
//!
//! Wiring is a byte comparison of one file: equal to what this binary
//! writes is wired; fufu's header with other bytes — a moved binary, an
//! older fufu — is wired and stale, the repair `ff hook -u` makes; a
//! `fufu.js` without the header is someone else's and is left alone.
//!
//! The config directory is the client's own rule: `$XDG_CONFIG_HOME/opencode`
//! when set, else `~/.config/opencode`, on every OS. `OPENCODE_CONFIG_DIR`
//! overrides it in OpenCode and is not read here.

use std::path::PathBuf;

use ff_core::Result;

use super::{
    AgentEvent, AgentProtocol, Change, EventKind, InstallOptions, Integration, Mechanism, Presence,
    Reply, Status, Wiring, payload, plugin, skill,
};

pub struct Opencode;

/// The plugin body, with one placeholder for the binary's path.
const PLUGIN: &str = include_str!("opencode.js");

/// The first line of the body: what makes a `fufu.js` fufu's.
const HEADER: &str = "// Written by `ff hook opencode`.";

const FILE: &str = "fufu.js";

const STANDING: &str = "the briefing is standing: in the system prompt on every model call";

// ---- paths -----------------------------------------------------------------

/// `$XDG_CONFIG_HOME/opencode` when set, else `~/.config/opencode` — the
/// client's own rule on every OS.
fn config_dir() -> Result<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(xdg) => PathBuf::from(xdg),
        None => super::home()?.join(".config"),
    };
    Ok(config.join("opencode"))
}

fn plugin_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("plugins").join(FILE))
}

fn skill_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("skills").join(skill::NAME))
}

/// The `opencode` binary: `FF_OPENCODE` when set, the test seam — a path
/// that is not a file means no OpenCode — else `opencode` (or its Windows
/// spellings) found on `PATH`.
fn opencode_binary() -> Option<PathBuf> {
    super::client_binary("FF_OPENCODE", &["opencode", "opencode.exe", "opencode.cmd"])
}

// ---- the plugin ------------------------------------------------------------

/// The body this binary writes: the placeholder replaced with the binary's
/// path as a JS string literal, so a Windows path's backslashes are
/// escaped.
fn plugin_body() -> String {
    let literal = serde_json::to_string(&super::exe_path()).expect("a string serializes");
    PLUGIN.replacen("__FF__", &literal, 1)
}

/// Byte-equal to the body this binary writes is wired; fufu's header with
/// other bytes — a moved binary, an older fufu — is wired and stale; a
/// `fufu.js` without the header is someone else's and is left alone.
fn plugin_wiring() -> (Wiring, bool) {
    let Ok(path) = plugin_path() else {
        return (Wiring::NotWired, false);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Wiring::NotWired, false);
    };
    let wired = Wiring::Wired {
        mechanism: Mechanism::Plugin,
        at: path.clone(),
    };
    if text == plugin_body() {
        (wired, false)
    } else if text.starts_with(HEADER) {
        (wired, true)
    } else {
        (Wiring::HandWritten { at: path }, false)
    }
}

fn skill_wiring() -> Wiring {
    match skill_dir() {
        Ok(dir) => skill::wiring(&dir),
        Err(_) => Wiring::NotWired,
    }
}

// ---- the integration -------------------------------------------------------

impl Integration for Opencode {
    fn slug(&self) -> &'static str {
        "opencode"
    }

    fn detect(&self) -> Presence {
        match config_dir() {
            Ok(dir) if dir.is_dir() => return Presence::Present { evidence: dir },
            _ => {}
        }
        match opencode_binary() {
            Some(binary) => Presence::Present { evidence: binary },
            None => Presence::Absent,
        }
    }

    fn status(&self) -> Status {
        let (wiring, stale) = plugin_wiring();
        Status {
            slug: self.slug(),
            presence: self.detect(),
            note: matches!(wiring, Wiring::Wired { .. }).then(|| STANDING.to_string()),
            wiring,
            parts: Vec::new(),
            skill: Some(skill_wiring()),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let path = plugin_path()?;
        if let (Wiring::HandWritten { .. }, _) = plugin_wiring() {
            return Err(super::failed(&path, "not fufu's file; move it aside"));
        }
        let mut written = plugin::write_if_changed(&path, &plugin_body())?;
        let dir = skill_dir()?;
        if !matches!(skill::wiring(&dir), Wiring::Wired { .. }) {
            skill::write(&dir)?;
            written = true;
        }
        match plugin_wiring() {
            (Wiring::Wired { .. }, false) => {}
            (verified, _) => {
                return Err(super::failed(
                    &path,
                    format!(
                        "the plugin did not verify after the write ({})",
                        verified.word()
                    ),
                ));
            }
        }
        let mut change = if written {
            Change::changed(format!("plugin written to {}", path.display()))
        } else {
            Change::unchanged(format!("already wired in {}", path.display()))
        };
        if written {
            change
                .lines
                .push(format!("skill written to {}", dir.display()));
        }
        change.lines.push(STANDING.into());
        if written {
            change.lines.push("restart OpenCode to load it".into());
        }
        Ok(change)
    }

    /// Exactly the two paths fufu wrote: the plugin file when it is
    /// fufu's — a file under fufu's name without fufu's header is someone
    /// else's and stays — and the skill directory.
    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let path = plugin_path()?;
        let mut change = match plugin_wiring() {
            (Wiring::Wired { .. }, _) => {
                std::fs::remove_file(&path).map_err(|err| super::failed(&path, err))?;
                Change::changed(format!("removed {}", path.display()))
            }
            (Wiring::HandWritten { .. }, _) => {
                Change::unchanged(format!("{} is not fufu's — left alone", path.display()))
            }
            _ => Change::unchanged("no fufu plugin installed"),
        };
        let dir = skill_dir()?;
        if skill::remove(&dir)? {
            change.absorb(Change::changed(format!("removed {}", dir.display())));
        }
        Ok(change)
    }

    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        Some(&Opencode)
    }
}

impl AgentProtocol for Opencode {
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
        let payload: payload::Payload = payload::parse_json(stdin)?;
        payload::to_event(&payload, forced)
    }

    /// The plugin carries the text into the system prompt itself, so the
    /// trigger prints it plain — and nothing on a tool, where the plugin
    /// discards the output: a coach line nobody reads would only stamp
    /// the marker.
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

    /// One placeholder in the source, none in the body, the binary's path
    /// as a JSON string literal in its place, the header first, and the
    /// three hooks, the variable, and both event names spelled.
    #[test]
    fn the_body_bakes_the_binary_as_a_string_literal() {
        assert_eq!(PLUGIN.matches("__FF__").count(), 1);
        let body = plugin_body();
        assert!(!body.contains("__FF__"), "{body}");
        let literal = serde_json::to_string(&super::super::exe_path()).unwrap();
        assert!(body.contains(&literal), "{body}");
        assert!(body.starts_with(HEADER), "{body}");
        for needle in [
            "\"shell.env\"",
            "\"experimental.chat.system.transform\"",
            "\"tool.execute.before\"",
            "OPENCODE_SESSION_ID",
            "trigger opencode",
            "tool_name: input.tool",
            "tool_input: output.args",
        ] {
            assert!(body.contains(needle), "{needle} in {body}");
        }
        // The two names the plugin's payloads spell: the standing briefing
        // is a boundary on every model call, and every tool call is the
        // floor. The body is the table, so this is what it has to name.
        for event in ["SessionStart", "PreToolUse"] {
            assert!(
                body.contains(&format!("hook_event_name: \"{event}\"")),
                "{event} in {body}"
            );
        }
    }

    /// The plugin's `PreToolUse` payload is the shared dialect: the bash
    /// tool's `command` is the label and the tally's input.
    #[test]
    fn the_tool_payload_reads_the_bash_command() {
        let event = Opencode
            .parse(
                br#"{"hook_event_name":"PreToolUse","session_id":"ses_1","cwd":"/repo",
                     "tool_name":"bash","tool_input":{"command":"cargo test","description":"run"}}"#,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, EventKind::BeforeTool);
        assert_eq!(event.session, "ses_1");
        assert_eq!(event.label, Label::text("bash(cargo test)"));
        assert_eq!(event.command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn the_briefing_goes_out_as_plain_text_and_a_tool_gets_nothing() {
        let mut reply = Reply::new(EventKind::SessionStart);
        reply.context.push("hello".into());
        assert_eq!(Opencode.reply_envelope(&reply).as_deref(), Some("hello"));

        let mut reply = Reply::new(EventKind::BeforeTool);
        reply.context.push("hello".into());
        assert!(Opencode.reply_envelope(&reply).is_none());
    }
}
