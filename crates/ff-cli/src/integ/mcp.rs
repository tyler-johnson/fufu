//! Removing the MCP server registration a fufu before v0.15 wrote.
//!
//! Through v0.14, `ff mcp` served fufu's verbs as typed tools over stdio,
//! and `ff hook <slug>` registered it with each client beside the capture
//! hook. The verb is gone, and a registration that still points at it
//! fails at every client start. This module is the migration: each
//! adapter's install and uninstall strip the entry the earlier fufu wrote,
//! so the next `ff hook`, `ff hook -u`, or `ff unhook` cleans a machine
//! wired before v0.15. Once such machines can be assumed rehooked — v0.17
//! is a fair horizon — this module can leave.
//!
//! Two shapes cover the three clients that carried one. Cursor and Gemini
//! take a JSON file with an `mcpServers` object, and the entry is the one
//! key `fufu` in it. Codex takes TOML, and fufu carries no TOML parser: the
//! registration was a block between two marker comments, and the markers
//! are what let it go whole.
//!
//! Ownership is recognized by the command's binary name and its arguments
//! — a binary called `ff` with the one argument `mcp` — rather than by the
//! whole path, so a moved binary still reads as fufu's. A `fufu` entry
//! that is not ours is hand-written, and is neither touched nor named.
//! Claude's registration lived inside the plugin directory, which install
//! rewrites whole and uninstall removes whole, so it needs nothing here.

use std::path::PathBuf;

use ff_core::{Error, Result};
use serde_json::Value;

use super::{Change, settings};

/// The key fufu's own server went under.
const NAME: &str = "fufu";

/// The binary the registration ran, by stem.
const BINARY: &str = "ff";

/// The one verb it ran.
const ARGS: [&str; 1] = ["mcp"];

/// How one client spells a server entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `mcpServers.fufu` in a JSON file.
    Json,
    /// `[mcp_servers.fufu]` in a TOML file, between marker comments.
    TomlBlock,
}

/// One client's file, and how the entry went into it.
pub struct Spec {
    pub path: PathBuf,
    pub shape: Shape,
}

/// Remove the registration an earlier fufu wrote, if the file carries one.
///
/// One line when something was removed, and silence otherwise: a machine
/// that never carried one, or one already cleaned, hears nothing on every
/// hook run, and a hand-written entry is left where it is.
pub fn strip(spec: &Spec) -> Result<Change> {
    match spec.shape {
        Shape::Json => json_strip(spec),
        Shape::TomlBlock => toml_strip(spec),
    }
}

fn silence() -> Change {
    Change {
        changed: false,
        lines: Vec::new(),
    }
}

fn removed(spec: &Spec) -> Change {
    Change::changed(format!("MCP server removed from {}", spec.path.display()))
}

/// A command's binary name: the file's stem, with or without the `.exe` a
/// Windows path carries. Both separators are split on, because a
/// registration written on Windows is read on Windows and the test that
/// pins this runs everywhere.
fn binary(command: &str) -> &str {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    name.strip_suffix(".exe").unwrap_or(name)
}

/// Whether a JSON entry is the one fufu registered: it runs the same
/// binary with the same arguments, whatever path that binary has moved to
/// since. The arguments are part of it because the command alone is a
/// general-purpose binary, and an entry running it for something else is
/// somebody's own.
fn entry_is_ours(entry: &Value) -> bool {
    entry["command"]
        .as_str()
        .is_some_and(|command| binary(command) == BINARY)
        && entry["args"] == serde_json::json!(ARGS)
}

// ---- the JSON key ----------------------------------------------------------

fn json_strip(spec: &Spec) -> Result<Change> {
    let mut settings = settings::load(&spec.path)?;
    let Some(entries) = settings
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
    else {
        return Ok(silence());
    };
    match entries.get(NAME) {
        Some(entry) if entry_is_ours(entry) => {}
        _ => return Ok(silence()),
    }
    entries.remove(NAME);
    if entries.is_empty() {
        settings.remove("mcpServers");
    }
    settings::write(&spec.path, &settings)?;
    Ok(removed(spec))
}

// ---- the TOML block --------------------------------------------------------

const BLOCK_BEGIN: &str = "# >>> fufu (ff hook codex) >>>";
const BLOCK_END: &str = "# <<< fufu <<<";

/// The span of fufu's block in a TOML file, as line indexes: the begin
/// marker through the end marker, inclusive of both.
fn block_span(lines: &[&str]) -> Option<(usize, usize)> {
    let begin = lines.iter().position(|line| line.trim() == BLOCK_BEGIN)?;
    let end = lines[begin..]
        .iter()
        .position(|line| line.trim() == BLOCK_END)
        .map(|offset| begin + offset)?;
    Some((begin, end))
}

/// The block is fufu's outright, so it goes whole, and a `[mcp_servers.fufu]`
/// table outside the markers is somebody's own.
fn toml_strip(spec: &Spec) -> Result<Change> {
    let Ok(contents) = std::fs::read_to_string(&spec.path) else {
        return Ok(silence());
    };
    let eol = super::shell::line_ending(&contents);
    let lines: Vec<&str> = contents.lines().collect();
    let Some((begin, end)) = block_span(&lines) else {
        return Ok(silence());
    };
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i < begin || *i > end)
        .map(|(_, line)| *line)
        .collect();
    let mut updated = kept.join(eol);
    if contents.ends_with('\n') && !updated.is_empty() {
        updated.push_str(eol);
    }
    std::fs::write(&spec.path, updated).map_err(Error::repo)?;
    Ok(removed(spec))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const TABLE: &str = "[mcp_servers.fufu]";

    fn json_spec(dir: &Path) -> Spec {
        Spec {
            path: dir.join("mcp.json"),
            shape: Shape::Json,
        }
    }

    fn toml_spec(dir: &Path) -> Spec {
        Spec {
            path: dir.join("config.toml"),
            shape: Shape::TomlBlock,
        }
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The v0.14 entry goes, the rest of the file stays, and a second pass
    /// says nothing.
    #[test]
    fn the_json_key_is_removed_and_the_rest_is_left() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path());
        std::fs::write(
            &spec.path,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"x"},"fufu":{"type":"stdio","command":"/old/place/ff","args":["mcp"]}}}"#,
        )
        .unwrap();
        let change = strip(&spec).unwrap();
        assert!(change.changed);
        assert_eq!(change.lines.len(), 1);
        assert!(change.lines[0].starts_with("MCP server removed from"));
        let after = read_json(&spec.path);
        assert_eq!(after["theme"], "dark");
        assert!(after["mcpServers"].get("fufu").is_none());
        assert_eq!(after["mcpServers"]["other"]["command"], "x");

        let change = strip(&spec).unwrap();
        assert!(!change.changed, "idempotent");
        assert!(change.lines.is_empty(), "{:?}", change.lines);
    }

    #[test]
    fn an_empty_servers_object_is_dropped_on_the_way_out() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path());
        std::fs::write(
            &spec.path,
            r#"{"mcpServers":{"fufu":{"command":"/x/ff","args":["mcp"]}}}"#,
        )
        .unwrap();
        assert!(strip(&spec).unwrap().changed);
        assert_eq!(read_json(&spec.path), serde_json::json!({}));
    }

    /// A file that never carried one, or that does not exist, is silence
    /// rather than a line about its absence.
    #[test]
    fn a_file_with_nothing_of_ours_is_silent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path());
        let change = strip(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.is_empty());
        assert!(!spec.path.exists(), "nothing is written for nothing");

        std::fs::write(&spec.path, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        let change = strip(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.is_empty());
    }

    #[test]
    fn a_hand_written_json_entry_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path());
        let mine = r#"{"mcpServers":{"fufu":{"command":"/my/wrapper.sh","args":["serve"]}}}"#;
        std::fs::write(&spec.path, mine).unwrap();
        let change = strip(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.is_empty(), "{:?}", change.lines);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
    }

    #[test]
    fn a_windows_binary_is_still_the_same_binary() {
        assert_eq!(binary("C:\\Users\\u\\bin\\ff.exe"), "ff");
        assert_eq!(binary("/usr/local/bin/ff"), "ff");
        assert_eq!(binary("ff"), "ff");
        assert_ne!(binary("/usr/local/bin/ffmpeg"), "ff");
        assert!(entry_is_ours(&serde_json::json!({
            "command": "C:\\bin\\ff.exe",
            "args": ["mcp"]
        })));
        assert!(!entry_is_ours(&serde_json::json!({
            "command": "/x/ff",
            "args": ["serve"]
        })));
        assert!(!entry_is_ours(&serde_json::json!({
            "command": "/x/ff"
        })));
    }

    #[test]
    fn the_toml_block_is_removed_and_the_rest_is_left() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        let rest = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        std::fs::write(
            &spec.path,
            format!(
                "{rest}{BLOCK_BEGIN}\n{TABLE}\ncommand = \"/old/ff\"\nargs = [\"mcp\"]\n{BLOCK_END}\n"
            ),
        )
        .unwrap();
        let change = strip(&spec).unwrap();
        assert!(change.changed);
        assert_eq!(change.lines.len(), 1);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), rest);

        let change = strip(&spec).unwrap();
        assert!(!change.changed, "idempotent");
        assert!(change.lines.is_empty());
    }

    /// A block in the middle of a file goes without disturbing either
    /// side, and a file that then holds nothing is left empty.
    #[test]
    fn the_toml_block_goes_from_the_middle_and_from_a_file_of_its_own() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        std::fs::write(
            &spec.path,
            format!("a = 1\n{BLOCK_BEGIN}\n{TABLE}\ncommand = \"/old/ff\"\nargs = [\"mcp\"]\n{BLOCK_END}\nb = 2\n"),
        )
        .unwrap();
        assert!(strip(&spec).unwrap().changed);
        assert_eq!(
            std::fs::read_to_string(&spec.path).unwrap(),
            "a = 1\nb = 2\n"
        );

        std::fs::write(
            &spec.path,
            format!(
                "{BLOCK_BEGIN}\n{TABLE}\ncommand = \"/old/ff\"\nargs = [\"mcp\"]\n{BLOCK_END}\n"
            ),
        )
        .unwrap();
        assert!(strip(&spec).unwrap().changed);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "");
    }

    #[test]
    fn a_hand_written_toml_table_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        let mine = "[mcp_servers.fufu]\ncommand = \"/my/wrapper.sh\"\n";
        std::fs::write(&spec.path, mine).unwrap();
        let change = strip(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.is_empty(), "{:?}", change.lines);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
    }

    #[test]
    fn a_missing_toml_file_is_silent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        let change = strip(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.is_empty());
        assert!(!spec.path.exists());
    }

    #[test]
    fn a_crlf_toml_file_keeps_its_line_endings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        std::fs::write(
            &spec.path,
            format!("a = 1\r\n{BLOCK_BEGIN}\r\n{TABLE}\r\ncommand = \"/old/ff\"\r\nargs = [\"mcp\"]\r\n{BLOCK_END}\r\n"),
        )
        .unwrap();
        assert!(strip(&spec).unwrap().changed);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "a = 1\r\n");
    }
}
