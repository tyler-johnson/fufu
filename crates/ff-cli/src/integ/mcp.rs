//! The MCP server's registration, per client.
//!
//! `ff mcp` is one tool over stdio, and a client has to be told where it
//! is. Each agent adapter registers it beside the capture hook it already
//! wires, so `ff hook <slug>` does both, `ff unhook <slug>` removes both,
//! and `ff doctor` reports both — with no new slug, because the server is
//! a property of a client fufu already integrates with and not a thing of
//! its own to wire.
//!
//! Two shapes cover the four clients. Three of them take a JSON file with
//! an `mcpServers` object, and fufu merges exactly one key into it —
//! `mcpServers.fufu` — leaving every other key as it was, on the same
//! engine and the same rules the hook entries use. Codex takes TOML, and
//! fufu carries no TOML parser: it appends a block between two marker
//! comments, the way the shells take marked lines in an rc file. A
//! `[table]` header appended to a TOML file is always valid, and the
//! markers are what let uninstall remove exactly the block and nothing
//! else.
//!
//! Ownership is recognized by the command's tail — a binary called `ff`
//! with the one argument `mcp` — rather than by the whole path, so a moved
//! binary still reads as registered. A registration that names `fufu` but
//! is not ours is hand-written, reported, and never touched.

use std::path::PathBuf;

use ff_core::{Error, Result};
use serde_json::{Map, Value};

use super::{Change, Mechanism, Wiring, settings};

/// The key each client's file carries the server under, and the name a
/// client prefixes the tool with: `mcp__fufu__ff`.
pub const NAME: &str = "fufu";

/// What the server is asked to run: this binary, and the one verb.
const ARGS: [&str; 1] = ["mcp"];

/// How one client spells a server entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `mcpServers.fufu = {"type": "stdio", "command": …, "args": ["mcp"]}`
    /// in a JSON file; `with_type` says whether the client wants the
    /// transport spelled out.
    Json { with_type: bool },
    /// `[mcp_servers.fufu]` in a TOML file, between marker comments.
    TomlBlock,
}

/// One client's file, and how the entry goes into it.
pub struct Spec {
    pub path: PathBuf,
    pub shape: Shape,
    /// The binary the client is told to run — this one, by absolute path,
    /// from [`super::exe_path`]. Carried rather than read here so the
    /// tests can name an `ff` that is not the test harness.
    pub command: String,
}

impl Spec {
    pub fn new(path: PathBuf, shape: Shape) -> Spec {
        Spec {
            path,
            shape,
            command: super::exe_path(),
        }
    }
}

// ---- the TOML block --------------------------------------------------------

const BLOCK_BEGIN: &str = "# >>> fufu (ff hook codex) >>>";
const BLOCK_END: &str = "# <<< fufu <<<";
const TABLE: &str = "[mcp_servers.fufu]";

/// Whether a command names this binary: the file's stem is `ff`, with or
/// without the `.exe` a Windows path carries. Both separators are split
/// on, because a registration written on Windows is read on Windows and
/// the test that pins this runs everywhere.
fn command_is_ff(command: &str) -> bool {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    stem == "ff"
}

/// The one entry install writes.
fn entry(spec: &Spec) -> Value {
    let mut map = Map::new();
    if spec.shape == (Shape::Json { with_type: true }) {
        map.insert("type".into(), "stdio".into());
    }
    map.insert("command".into(), spec.command.as_str().into());
    map.insert("args".into(), serde_json::json!(ARGS));
    Value::Object(map)
}

/// Whether a JSON entry is fufu's: it runs a binary called `ff` with the
/// one argument `mcp`, whatever path the binary has moved to since.
fn entry_is_ours(entry: &Value) -> bool {
    entry["command"].as_str().is_some_and(command_is_ff) && entry["args"] == serde_json::json!(ARGS)
}

/// The TOML block, as install writes it. The command is a TOML basic
/// string, which shares JSON's escapes for everything a path can hold, so
/// the JSON encoder spells it.
fn block(spec: &Spec, eol: &str) -> String {
    let command = serde_json::to_string(&spec.command).unwrap_or_default();
    [
        BLOCK_BEGIN,
        TABLE,
        &format!("command = {command}"),
        "args = [\"mcp\"]",
        BLOCK_END,
    ]
    .join(eol)
        + eol
}

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

/// Whether the table header appears outside fufu's block — somebody wrote
/// the registration themselves.
fn toml_hand_written(lines: &[&str]) -> bool {
    let span = block_span(lines);
    lines.iter().enumerate().any(|(i, line)| {
        line.trim() == TABLE && !span.is_some_and(|(begin, end)| i >= begin && i <= end)
    })
}

fn toml_wiring(spec: &Spec) -> Wiring {
    let contents = match std::fs::read_to_string(&spec.path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Wiring::NotWired,
        Err(err) => return Wiring::Unavailable(format!("{}: {err}", spec.path.display())),
    };
    let lines: Vec<&str> = contents.lines().collect();
    if let Some((begin, end)) = block_span(&lines)
        && lines[begin..=end].iter().any(|line| line.trim() == TABLE)
    {
        return Wiring::Wired {
            mechanism: Mechanism::Settings,
            at: spec.path.clone(),
        };
    }
    if toml_hand_written(&lines) {
        return Wiring::HandWritten;
    }
    Wiring::NotWired
}

fn toml_install(spec: &Spec) -> Result<Change> {
    let original = std::fs::read_to_string(&spec.path).unwrap_or_default();
    let eol = super::shell::line_ending(&original);
    let lines: Vec<&str> = original.lines().collect();
    if toml_hand_written(&lines) && block_span(&lines).is_none() {
        return Ok(Change::unchanged(format!(
            "{} already registers the MCP server by hand — leaving it alone",
            spec.path.display()
        )));
    }
    let wanted = block(spec, eol);
    let mut kept: Vec<String> = Vec::new();
    let mut had_block = false;
    match block_span(&lines) {
        Some((begin, end)) => {
            had_block = true;
            let current = lines[begin..=end]
                .iter()
                .map(|line| format!("{line}{eol}"))
                .collect::<String>();
            if current == wanted {
                return Ok(Change::unchanged(format!(
                    "MCP server already registered in {}",
                    spec.path.display()
                )));
            }
            kept.extend(lines[..begin].iter().map(|line| line.to_string()));
            kept.push(wanted.trim_end_matches(eol).to_string());
            kept.extend(lines[end + 1..].iter().map(|line| line.to_string()));
        }
        None => {
            kept.extend(lines.iter().map(|line| line.to_string()));
            kept.push(wanted.trim_end_matches(eol).to_string());
        }
    }
    let mut updated = kept.join(eol);
    updated.push_str(eol);
    if let Some(parent) = spec.path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    std::fs::write(&spec.path, updated).map_err(Error::repo)?;
    Ok(Change::changed(format!(
        "MCP server {} in {}",
        if had_block {
            "re-registered"
        } else {
            "registered"
        },
        spec.path.display()
    )))
}

fn toml_uninstall(spec: &Spec) -> Result<Change> {
    let Ok(contents) = std::fs::read_to_string(&spec.path) else {
        return Ok(Change::unchanged(format!(
            "no MCP server registered in {}",
            spec.path.display()
        )));
    };
    let eol = super::shell::line_ending(&contents);
    let lines: Vec<&str> = contents.lines().collect();
    let Some((begin, end)) = block_span(&lines) else {
        let mut change = Change::unchanged(format!(
            "no MCP server registered in {}",
            spec.path.display()
        ));
        if toml_hand_written(&lines) {
            change.lines.push(format!(
                "the MCP server in {} was registered by hand — not touching it",
                spec.path.display()
            ));
        }
        return Ok(change);
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
    Ok(Change::changed(format!(
        "MCP server removed from {}",
        spec.path.display()
    )))
}

// ---- the JSON key ----------------------------------------------------------

fn json_entry(settings: &Map<String, Value>) -> Option<&Value> {
    settings.get("mcpServers")?.as_object()?.get(NAME)
}

fn json_wiring(spec: &Spec) -> Wiring {
    let settings = match settings::load(&spec.path) {
        Ok(settings) => settings,
        Err(err) => return Wiring::Unavailable(err.to_string()),
    };
    match json_entry(&settings) {
        None => Wiring::NotWired,
        Some(entry) if entry_is_ours(entry) => Wiring::Wired {
            mechanism: Mechanism::Settings,
            at: spec.path.clone(),
        },
        Some(_) => Wiring::HandWritten,
    }
}

fn json_install(spec: &Spec) -> Result<Change> {
    let mut settings = settings::load(&spec.path)?;
    let wanted = entry(spec);
    let servers = settings
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let servers = servers.as_object_mut().ok_or_else(|| {
        Error::msg(format!(
            "{}: \"mcpServers\" is not an object; file untouched",
            spec.path.display()
        ))
    })?;
    match servers.get(NAME) {
        Some(current) if *current == wanted => {
            return Ok(Change::unchanged(format!(
                "MCP server already registered in {}",
                spec.path.display()
            )));
        }
        Some(current) if !entry_is_ours(current) => {
            return Ok(Change::unchanged(format!(
                "{} already registers the MCP server by hand — leaving it alone",
                spec.path.display()
            )));
        }
        _ => {}
    }
    let had = servers.insert(NAME.to_string(), wanted).is_some();
    settings::write(&spec.path, &settings)?;
    Ok(Change::changed(format!(
        "MCP server {} in {}",
        if had { "re-registered" } else { "registered" },
        spec.path.display()
    )))
}

fn json_uninstall(spec: &Spec) -> Result<Change> {
    let mut settings = settings::load(&spec.path)?;
    let Some(servers) = settings
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
    else {
        return Ok(Change::unchanged(format!(
            "no MCP server registered in {}",
            spec.path.display()
        )));
    };
    match servers.get(NAME) {
        None => {
            return Ok(Change::unchanged(format!(
                "no MCP server registered in {}",
                spec.path.display()
            )));
        }
        Some(current) if !entry_is_ours(current) => {
            let mut change = Change::unchanged(format!(
                "no MCP server registered in {}",
                spec.path.display()
            ));
            change.lines.push(format!(
                "the MCP server in {} was registered by hand — not touching it",
                spec.path.display()
            ));
            return Ok(change);
        }
        Some(_) => {}
    }
    servers.remove(NAME);
    if servers.is_empty() {
        settings.remove("mcpServers");
    }
    settings::write(&spec.path, &settings)?;
    Ok(Change::changed(format!(
        "MCP server removed from {}",
        spec.path.display()
    )))
}

// ---- the three verbs -------------------------------------------------------

/// The one answer `ff hook -l` and `ff doctor` both read.
pub fn wiring(spec: &Spec) -> Wiring {
    match spec.shape {
        Shape::Json { .. } => json_wiring(spec),
        Shape::TomlBlock => toml_wiring(spec),
    }
}

pub fn install(spec: &Spec) -> Result<Change> {
    match spec.shape {
        Shape::Json { .. } => json_install(spec),
        Shape::TomlBlock => toml_install(spec),
    }
}

pub fn uninstall(spec: &Spec) -> Result<Change> {
    match spec.shape {
        Shape::Json { .. } => json_uninstall(spec),
        Shape::TomlBlock => toml_uninstall(spec),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const EXE: &str = "/opt/fufu/bin/ff";

    fn json_spec(dir: &Path, with_type: bool) -> Spec {
        Spec {
            path: dir.join("mcp.json"),
            shape: Shape::Json { with_type },
            command: EXE.into(),
        }
    }

    fn toml_spec(dir: &Path) -> Spec {
        Spec {
            path: dir.join("config.toml"),
            shape: Shape::TomlBlock,
            command: EXE.into(),
        }
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn the_json_key_round_trips_and_leaves_the_rest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path(), true);
        std::fs::write(
            &spec.path,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(wiring(&spec), Wiring::NotWired);

        assert!(install(&spec).unwrap().changed);
        assert!(!install(&spec).unwrap().changed, "idempotent");
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));
        let after = read_json(&spec.path);
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["mcpServers"]["other"]["command"], "x");
        assert_eq!(after["mcpServers"]["fufu"]["type"], "stdio");
        assert_eq!(
            after["mcpServers"]["fufu"]["args"],
            serde_json::json!(["mcp"])
        );
        assert_eq!(after["mcpServers"]["fufu"]["command"], EXE);

        assert!(uninstall(&spec).unwrap().changed);
        assert!(!uninstall(&spec).unwrap().changed);
        let after = read_json(&spec.path);
        assert_eq!(after["theme"], "dark");
        assert!(after["mcpServers"].get("fufu").is_none());
        assert_eq!(after["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn an_empty_servers_object_is_dropped_on_the_way_out() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path(), false);
        assert!(install(&spec).unwrap().changed);
        let after = read_json(&spec.path);
        assert!(
            after["mcpServers"]["fufu"].get("type").is_none(),
            "gemini's shape carries no type"
        );
        assert!(uninstall(&spec).unwrap().changed);
        assert_eq!(read_json(&spec.path), serde_json::json!({}));
    }

    /// A moved binary still reads as registered, and the next install
    /// rewrites the path rather than adding a second entry.
    #[test]
    fn a_moved_binary_is_still_ours_and_is_rewired() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path(), true);
        std::fs::write(
            &spec.path,
            r#"{"mcpServers":{"fufu":{"type":"stdio","command":"/old/place/ff","args":["mcp"]}}}"#,
        )
        .unwrap();
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));
        assert!(install(&spec).unwrap().changed, "the path is rewritten");
        let after = read_json(&spec.path);
        assert_eq!(after["mcpServers"]["fufu"]["command"], EXE);
        assert_eq!(after["mcpServers"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn a_hand_written_json_entry_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = json_spec(tmp.path(), true);
        let mine = r#"{"mcpServers":{"fufu":{"command":"/my/wrapper.sh","args":["serve"]}}}"#;
        std::fs::write(&spec.path, mine).unwrap();
        assert_eq!(wiring(&spec), Wiring::HandWritten);
        let change = install(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines[0].contains("by hand"), "{:?}", change.lines);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
        let change = uninstall(&spec).unwrap();
        assert!(!change.changed);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
    }

    #[test]
    fn a_windows_binary_is_still_ff() {
        assert!(command_is_ff("C:\\Users\\u\\bin\\ff.exe"));
        assert!(command_is_ff("/usr/local/bin/ff"));
        assert!(command_is_ff("ff"));
        assert!(!command_is_ff("/usr/local/bin/ffmpeg"));
        assert!(!entry_is_ours(
            &serde_json::json!({"command": "/x/ff", "args": ["serve"]})
        ));
    }

    #[test]
    fn the_toml_block_round_trips_and_leaves_the_rest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        std::fs::write(
            &spec.path,
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();
        assert_eq!(wiring(&spec), Wiring::NotWired);

        assert!(install(&spec).unwrap().changed);
        assert!(!install(&spec).unwrap().changed, "idempotent");
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(after.starts_with("model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n"));
        assert!(after.contains(&format!("{BLOCK_BEGIN}\n{TABLE}\ncommand = \"{EXE}\"\n")));
        assert!(
            after.ends_with("args = [\"mcp\"]\n# <<< fufu <<<\n"),
            "{after}"
        );

        assert!(uninstall(&spec).unwrap().changed);
        assert!(!uninstall(&spec).unwrap().changed);
        assert_eq!(
            std::fs::read_to_string(&spec.path).unwrap(),
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n"
        );
    }

    #[test]
    fn a_hand_written_toml_table_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        let mine = "[mcp_servers.fufu]\ncommand = \"/my/wrapper.sh\"\n";
        std::fs::write(&spec.path, mine).unwrap();
        assert_eq!(wiring(&spec), Wiring::HandWritten);
        assert!(!install(&spec).unwrap().changed);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
        let change = uninstall(&spec).unwrap();
        assert!(!change.changed);
        assert!(change.lines.iter().any(|l| l.contains("by hand")));
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), mine);
    }

    /// A block written by a fufu that lived elsewhere is rewritten in
    /// place, once, and a file with no trailing newline gains one before
    /// the block rather than having the block glued onto its last line.
    #[test]
    fn the_toml_block_is_rewritten_in_place_and_appended_cleanly() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        std::fs::write(
            &spec.path,
            format!("a = 1\n{BLOCK_BEGIN}\n{TABLE}\ncommand = \"/old/ff\"\nargs = [\"mcp\"]\n{BLOCK_END}\nb = 2\n"),
        )
        .unwrap();
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));
        assert!(install(&spec).unwrap().changed);
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(after.starts_with("a = 1\n"));
        assert!(after.ends_with("b = 2\n"));
        assert_eq!(after.matches(BLOCK_BEGIN).count(), 1);
        assert!(!after.contains("/old/ff"));

        std::fs::write(&spec.path, "a = 1").unwrap();
        assert!(install(&spec).unwrap().changed);
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(after.starts_with("a = 1\n# >>> fufu"), "{after}");
    }

    #[test]
    fn a_crlf_toml_file_keeps_its_line_endings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = toml_spec(tmp.path());
        std::fs::write(&spec.path, "a = 1\r\n").unwrap();
        assert!(install(&spec).unwrap().changed);
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(after.contains("\r\n"));
        assert!(!after.replace("\r\n", "").contains('\n'), "{after:?}");
        assert!(uninstall(&spec).unwrap().changed);
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "a = 1\r\n");
    }
}
