//! The MCP servers' registration, per client.
//!
//! `ff mcp` is one tool over stdio, and a client has to be told where it
//! is. Each agent adapter registers it beside the capture hook it already
//! wires, so `ff hook <slug>` does both, `ff unhook <slug>` removes both,
//! and `ff doctor` reports both — with no new slug, because the server is
//! a property of a client fufu already integrates with and not a thing of
//! its own to wire.
//!
//! Two shapes cover the four clients. Three of them take a JSON file with
//! an `mcpServers` object, and fufu merges one key per server into it,
//! leaving every other key as it was, on the same engine and the same
//! rules the hook entries use. Codex takes TOML, and fufu carries no TOML
//! parser: it appends a block between two marker comments, the way the
//! shells take marked lines in an rc file. A `[table]` header appended to
//! a TOML file is always valid, and the markers are what let uninstall
//! remove exactly the block and nothing else.
//!
//! A server's identity is a value rather than a constant, because there is
//! more than one of them. fufu's own is `mcpServers.fufu` running this
//! binary with the one verb. A declared extension whose manifest names an
//! MCP server gets `mcpServers.<name>` beside it, under the bare name the
//! rest of its namespace hangs off, and Codex's one marked block carries a
//! table apiece.
//!
//! Ownership is recognized by the command's binary name and its arguments
//! — for fufu's own, a binary called `ff` with the one argument `mcp` —
//! rather than by the whole path, so a moved binary still reads as
//! registered. A registration under one of these names that is not ours is
//! hand-written, reported, and never touched.
//!
//! The registry is what says a server belongs in the file at all, which is
//! what makes an extension taken back with `ff extension remove` a case
//! rather than a question: the next `ff hook` writes Codex's block whole
//! and the table goes with it, and the JSON key is left behind the way the
//! manifest's other traces are once nothing reads the registry for that
//! name.

use std::path::PathBuf;

use ff_core::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{Change, Mechanism, Wiring, settings};

/// The key fufu's own server goes under, and the name a client prefixes
/// the tool with: `mcp__fufu__ff`.
pub const NAME: &str = "fufu";

/// What fufu's own server is asked to run: this binary, and the one verb.
const ARGS: [&str; 1] = ["mcp"];

/// How one client spells a server entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `mcpServers.<name> = {"type": "stdio", "command": …, "args": […]}`
    /// in a JSON file; `with_type` says whether the client wants the
    /// transport spelled out.
    Json { with_type: bool },
    /// `[mcp_servers.<name>]` in a TOML file, between marker comments.
    TomlBlock,
}

/// One server, as a client's file carries it.
///
/// The name is the key in the JSON object and the table in Codex's TOML:
/// `fufu` for fufu's own, and a declared extension's own name for one of
/// its. An extension's name is ASCII alphanumeric with `-` and `_`, which
/// is a bare TOML key and needs no quoting.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// The environment the client runs it with, empty when it names none.
    /// MCP types this a string map, so a manifest value that is not a
    /// string is spelled as its JSON text rather than dropped — every
    /// client then reads the same value out of its own file.
    pub env: Map<String, Value>,
}

impl Server {
    /// fufu's own: this binary, and the one verb.
    pub fn own(command: String) -> Server {
        Server {
            name: NAME.to_string(),
            command,
            args: ARGS.iter().map(|arg| arg.to_string()).collect(),
            env: Map::new(),
        }
    }

    /// A declared extension's own, keyed by the extension's name — the
    /// namespace everything else about it hangs off, and what
    /// `machine-surface.md` promises a client's file carries it under.
    fn extension(name: &str, mcp: &crate::manifest::McpServer) -> Server {
        let env = mcp
            .env
            .iter()
            .flatten()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                (key.clone(), Value::String(value))
            })
            .collect();
        Server {
            name: name.to_string(),
            command: mcp.command.clone(),
            args: mcp.args.clone(),
            env,
        }
    }

    /// How a change line names this server at the head of a sentence.
    fn heading(&self) -> String {
        if self.name == NAME {
            "MCP server".to_string()
        } else {
            format!("{}'s MCP server", self.name)
        }
    }

    /// The same, mid-sentence, carrying its own article.
    fn phrase(&self) -> String {
        if self.name == NAME {
            "the MCP server".to_string()
        } else {
            self.heading()
        }
    }
}

/// Every server a hooked client's file should carry: fufu's own, then one
/// per declared extension whose manifest names one, in registry order.
///
/// An extension declaring itself `fufu` is skipped rather than allowed to
/// take the key fufu's own server answers under.
fn servers(command: String) -> Vec<Server> {
    let mut servers = vec![Server::own(command)];
    for declared in crate::registry::read().declared() {
        if let Some(mcp) = &declared.manifest.mcp
            && declared.name() != NAME
        {
            servers.push(Server::extension(declared.name(), mcp));
        }
    }
    servers
}

/// One client's file, and how the entries go into it.
pub struct Spec {
    pub path: PathBuf,
    pub shape: Shape,
    /// Every server the file should carry, fufu's own first. The command
    /// each names is carried rather than read here so the tests can name
    /// an `ff` that is not the test harness.
    pub servers: Vec<Server>,
}

impl Spec {
    pub fn new(path: PathBuf, shape: Shape) -> Spec {
        Spec {
            path,
            shape,
            servers: servers(super::exe_path()),
        }
    }

    /// fufu's own server, which is the one `ff hook -l` and `ff doctor`
    /// report the wiring of.
    fn own(&self) -> Option<&Server> {
        self.servers.iter().find(|server| server.name == NAME)
    }
}

// ---- the TOML block --------------------------------------------------------

const BLOCK_BEGIN: &str = "# >>> fufu (ff hook codex) >>>";
const BLOCK_END: &str = "# <<< fufu <<<";

/// The table header one server takes.
fn table(server: &Server) -> String {
    format!("[mcp_servers.{}]", server.name)
}

/// A command's binary name: the file's stem, with or without the `.exe` a
/// Windows path carries. Both separators are split on, because a
/// registration written on Windows is read on Windows and the test that
/// pins this runs everywhere.
fn binary(command: &str) -> &str {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    name.strip_suffix(".exe").unwrap_or(name)
}

/// The entry install writes for one server.
fn entry(shape: Shape, server: &Server) -> Value {
    let mut map = Map::new();
    if shape == (Shape::Json { with_type: true }) {
        map.insert("type".into(), "stdio".into());
    }
    map.insert("command".into(), server.command.as_str().into());
    map.insert("args".into(), serde_json::json!(server.args));
    if !server.env.is_empty() {
        map.insert("env".into(), Value::Object(server.env.clone()));
    }
    Value::Object(map)
}

/// Whether a JSON entry is the one fufu registered: it runs the same
/// binary with the same arguments, whatever path that binary has moved to
/// since. The arguments are part of it because the command alone is a
/// general-purpose binary — `ff mcp` and `ff tower serve` are the same
/// `ff` — and an entry running it for something else is somebody's own.
fn entry_is_ours(server: &Server, entry: &Value) -> bool {
    entry["command"]
        .as_str()
        .is_some_and(|command| binary(command) == binary(&server.command))
        && entry["args"] == serde_json::json!(server.args)
}

/// A TOML basic string, which shares JSON's escapes for everything a path
/// or an environment value can hold, so the JSON encoder spells it.
fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn toml_array(values: &[String]) -> String {
    let values: Vec<String> = values.iter().map(|value| toml_string(value)).collect();
    format!("[{}]", values.join(", "))
}

/// An inline table. The keys are quoted, which is valid TOML whatever an
/// environment variable is called.
fn toml_table(env: &Map<String, Value>) -> String {
    let pairs: Vec<String> = env
        .iter()
        .map(|(key, value)| {
            format!(
                "{} = {}",
                toml_string(key),
                toml_string(value.as_str().unwrap_or_default())
            )
        })
        .collect();
    format!("{{ {} }}", pairs.join(", "))
}

/// The block, as install writes it: the two markers, and a table apiece
/// for the servers between them.
fn block(servers: &[&Server], eol: &str) -> String {
    let mut lines = vec![BLOCK_BEGIN.to_string()];
    for server in servers {
        lines.push(table(server));
        lines.push(format!("command = {}", toml_string(&server.command)));
        lines.push(format!("args = {}", toml_array(&server.args)));
        if !server.env.is_empty() {
            lines.push(format!("env = {}", toml_table(&server.env)));
        }
    }
    lines.push(BLOCK_END.to_string());
    lines.join(eol) + eol
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

/// The table headers inside fufu's block, trimmed.
fn block_tables(lines: &[&str]) -> Vec<String> {
    match block_span(lines) {
        Some((begin, end)) => lines[begin..=end]
            .iter()
            .map(|line| line.trim().to_string())
            .collect(),
        None => Vec::new(),
    }
}

/// The servers whose table header appears outside fufu's block and not
/// inside it — somebody wrote that registration themselves. A header that
/// also stands inside the block is fufu's, and the duplicate outside is
/// left where it is.
fn toml_hand_written<'a>(lines: &[&str], servers: &'a [Server]) -> Vec<&'a str> {
    let span = block_span(lines);
    let inside = |i: usize| span.is_some_and(|(begin, end)| i >= begin && i <= end);
    servers
        .iter()
        .filter(|server| {
            let header = table(server);
            let found: Vec<bool> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.trim() == header)
                .map(|(i, _)| inside(i))
                .collect();
            !found.is_empty() && !found.iter().any(|is_ours| *is_ours)
        })
        .map(|server| server.name.as_str())
        .collect()
}

fn toml_wiring(spec: &Spec) -> Wiring {
    let Some(own) = spec.own() else {
        return Wiring::NotWired;
    };
    let contents = match std::fs::read_to_string(&spec.path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Wiring::NotWired,
        Err(err) => return Wiring::Unavailable(format!("{}: {err}", spec.path.display())),
    };
    let lines: Vec<&str> = contents.lines().collect();
    if block_tables(&lines).contains(&table(own)) {
        return Wiring::Wired {
            mechanism: Mechanism::Settings,
            at: spec.path.clone(),
        };
    }
    if !toml_hand_written(&lines, std::slice::from_ref(own)).is_empty() {
        return Wiring::HandWritten;
    }
    Wiring::NotWired
}

fn toml_install(spec: &Spec) -> Result<Change> {
    let original = std::fs::read_to_string(&spec.path).unwrap_or_default();
    let eol = super::shell::line_ending(&original);
    let lines: Vec<&str> = original.lines().collect();
    let hand = toml_hand_written(&lines, &spec.servers);
    let (ours, theirs): (Vec<&Server>, Vec<&Server>) = spec
        .servers
        .iter()
        .partition(|server| !hand.contains(&server.name.as_str()));
    let mut change = Change {
        changed: false,
        lines: Vec::new(),
    };
    for server in &theirs {
        change.lines.push(format!(
            "{} already registers {} by hand — leaving it alone",
            spec.path.display(),
            server.phrase()
        ));
    }
    if ours.is_empty() {
        return Ok(change);
    }
    let wanted = block(&ours, eol);
    let had = block_tables(&lines);
    let mut kept: Vec<String> = Vec::new();
    match block_span(&lines) {
        Some((begin, end)) => {
            let current = lines[begin..=end]
                .iter()
                .map(|line| format!("{line}{eol}"))
                .collect::<String>();
            if current == wanted {
                for server in &ours {
                    change.lines.push(format!(
                        "{} already registered in {}",
                        server.heading(),
                        spec.path.display()
                    ));
                }
                return Ok(change);
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
    change.changed = true;
    for server in &ours {
        change.lines.push(format!(
            "{} {} in {}",
            server.heading(),
            if had.contains(&table(server)) {
                "re-registered"
            } else {
                "registered"
            },
            spec.path.display()
        ));
    }
    Ok(change)
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
        for name in toml_hand_written(&lines, &spec.servers) {
            let server = spec
                .servers
                .iter()
                .find(|server| server.name == name)
                .expect("named from this list");
            change.lines.push(format!(
                "{} in {} was registered by hand — not touching it",
                server.phrase(),
                spec.path.display()
            ));
        }
        return Ok(change);
    };
    // The block is fufu's outright, so it goes whole — including a table
    // for an extension the registry has since forgotten, which is why the
    // lines below name only the servers still declared.
    let had = block_tables(&lines);
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
    let mut change = Change::changed(format!("MCP server removed from {}", spec.path.display()));
    for server in &spec.servers {
        if server.name != NAME && had.contains(&table(server)) {
            change.lines.push(format!(
                "{} removed from {}",
                server.heading(),
                spec.path.display()
            ));
        }
    }
    Ok(change)
}

// ---- the JSON keys ---------------------------------------------------------

fn json_entry<'a>(settings: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    settings.get("mcpServers")?.as_object()?.get(name)
}

fn json_wiring(spec: &Spec) -> Wiring {
    let Some(own) = spec.own() else {
        return Wiring::NotWired;
    };
    let settings = match settings::load(&spec.path) {
        Ok(settings) => settings,
        Err(err) => return Wiring::Unavailable(err.to_string()),
    };
    match json_entry(&settings, &own.name) {
        None => Wiring::NotWired,
        Some(entry) if entry_is_ours(own, entry) => Wiring::Wired {
            mechanism: Mechanism::Settings,
            at: spec.path.clone(),
        },
        Some(_) => Wiring::HandWritten,
    }
}

fn json_install(spec: &Spec) -> Result<Change> {
    let mut settings = settings::load(&spec.path)?;
    let entries = settings
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let entries = entries.as_object_mut().ok_or_else(|| {
        Error::msg(format!(
            "{}: \"mcpServers\" is not an object; file untouched",
            spec.path.display()
        ))
    })?;
    let mut change = Change {
        changed: false,
        lines: Vec::new(),
    };
    for server in &spec.servers {
        let wanted = entry(spec.shape, server);
        match entries.get(&server.name) {
            Some(current) if *current == wanted => {
                change.lines.push(format!(
                    "{} already registered in {}",
                    server.heading(),
                    spec.path.display()
                ));
                continue;
            }
            Some(current) if !entry_is_ours(server, current) => {
                change.lines.push(format!(
                    "{} already registers {} by hand — leaving it alone",
                    spec.path.display(),
                    server.phrase()
                ));
                continue;
            }
            _ => {}
        }
        let had = entries.insert(server.name.clone(), wanted).is_some();
        change.changed = true;
        change.lines.push(format!(
            "{} {} in {}",
            server.heading(),
            if had { "re-registered" } else { "registered" },
            spec.path.display()
        ));
    }
    if change.changed {
        settings::write(&spec.path, &settings)?;
    }
    Ok(change)
}

fn json_uninstall(spec: &Spec) -> Result<Change> {
    let mut settings = settings::load(&spec.path)?;
    let Some(entries) = settings
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
    else {
        return Ok(Change::unchanged(format!(
            "no MCP server registered in {}",
            spec.path.display()
        )));
    };
    let mut change = Change {
        changed: false,
        lines: Vec::new(),
    };
    for server in &spec.servers {
        match entries.get(&server.name) {
            // Only fufu's own absence is news. A client hooked before an
            // extension was declared never carried its server, and saying
            // so on every unhook is noise.
            None => {
                if server.name == NAME {
                    change.lines.push(format!(
                        "no MCP server registered in {}",
                        spec.path.display()
                    ));
                }
            }
            Some(current) if !entry_is_ours(server, current) => {
                change.lines.push(format!(
                    "{} in {} was registered by hand — not touching it",
                    server.phrase(),
                    spec.path.display()
                ));
            }
            Some(_) => {
                entries.remove(&server.name);
                change.changed = true;
                change.lines.push(format!(
                    "{} removed from {}",
                    server.heading(),
                    spec.path.display()
                ));
            }
        }
    }
    if !change.changed {
        return Ok(change);
    }
    if entries.is_empty() {
        settings.remove("mcpServers");
    }
    settings::write(&spec.path, &settings)?;
    Ok(change)
}

// ---- the three verbs -------------------------------------------------------

/// The one answer `ff hook -l` and `ff doctor` both read, which is about
/// fufu's own server: a declared extension's is reported on its own.
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

// ---- what doctor reads about a declared extension's own server ------------

/// A declared extension's own server, as one client's file carries it —
/// [`Wiring`] with one more distinction `doctor` needs and nothing else
/// does: an entry that runs the same binary a manifest names but with
/// arguments the manifest has since moved past. It reads as [`HandWritten`]
/// to [`entry_is_ours`], and install and uninstall are right to leave it
/// alone either way, but doctor can still tell the two apart, because the
/// binary is still this server's own.
///
/// [`Wiring`]: super::Wiring
/// [`HandWritten`]: super::Wiring::HandWritten
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ServerWiring {
    NotWired,
    Wired {
        at: PathBuf,
    },
    /// Same binary, different arguments — the shape a registration takes
    /// once a manifest's `mcp.args` changes without the client's file
    /// being rewritten. `ff hook <slug>` will not overwrite it: the
    /// ownership test that would let a repair rewrite an entry is exactly
    /// the one this fails.
    Stale {
        at: PathBuf,
    },
    HandWritten,
    Unavailable(String),
}

impl ServerWiring {
    /// Where the entry lives, when it is there at all.
    pub fn at(&self) -> Option<&std::path::Path> {
        match self {
            ServerWiring::Wired { at } | ServerWiring::Stale { at } => Some(at),
            _ => None,
        }
    }
}

/// Whether an entry runs the server's own binary with arguments that no
/// longer match — [`entry_is_ours`] with the args check inverted, so the
/// two are mutually exclusive by construction.
fn entry_is_stale(server: &Server, entry: &Value) -> bool {
    entry["command"]
        .as_str()
        .is_some_and(|command| binary(command) == binary(&server.command))
        && entry["args"] != serde_json::json!(server.args)
}

fn json_ext_wiring(spec: &Spec, server: &Server) -> ServerWiring {
    let settings = match settings::load(&spec.path) {
        Ok(settings) => settings,
        Err(err) => return ServerWiring::Unavailable(err.to_string()),
    };
    match json_entry(&settings, &server.name) {
        None => ServerWiring::NotWired,
        Some(entry) if entry_is_ours(server, entry) => ServerWiring::Wired {
            at: spec.path.clone(),
        },
        Some(entry) if entry_is_stale(server, entry) => ServerWiring::Stale {
            at: spec.path.clone(),
        },
        Some(_) => ServerWiring::HandWritten,
    }
}

/// The Codex block only ever answers `Wired` or `HandWritten` for a table
/// it carries: install rewrites the whole block from the current registry
/// on every run, so a table inside it is never behind what the manifest
/// says for longer than one `ff hook codex`, the same limitation
/// [`toml_wiring`] already accepts for fufu's own table.
fn toml_ext_wiring(spec: &Spec, server: &Server) -> ServerWiring {
    let contents = match std::fs::read_to_string(&spec.path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ServerWiring::NotWired,
        Err(err) => return ServerWiring::Unavailable(format!("{}: {err}", spec.path.display())),
    };
    let lines: Vec<&str> = contents.lines().collect();
    if block_tables(&lines).contains(&table(server)) {
        return ServerWiring::Wired {
            at: spec.path.clone(),
        };
    }
    if !toml_hand_written(&lines, std::slice::from_ref(server)).is_empty() {
        return ServerWiring::HandWritten;
    }
    ServerWiring::NotWired
}

/// The state of one server — fufu's own or a declared extension's — in one
/// client's file. `name` is looked up in `spec.servers`, which already
/// carries every currently declared extension beside fufu's; a name not
/// found there (nothing declares it) answers `NotWired` rather than
/// panicking, though doctor calls this only with a name it just got from
/// the registry.
pub fn extension_wiring(spec: &Spec, name: &str) -> ServerWiring {
    let Some(server) = spec.servers.iter().find(|server| server.name == name) else {
        return ServerWiring::NotWired;
    };
    match spec.shape {
        Shape::Json { .. } => json_ext_wiring(spec, server),
        Shape::TomlBlock => toml_ext_wiring(spec, server),
    }
}

/// Every name this client's file registers a server under that is neither
/// fufu's own nor any currently declared extension's — the trace `ff
/// extension remove` leaves behind. Codex's is transient: the next `ff
/// hook codex` rewrites the block whole and the table goes with it. A JSON
/// key has no such moment and sits there until somebody removes it by
/// hand, which is what makes it worth doctor naming.
fn orphaned(spec: &Spec) -> Vec<String> {
    let known: Vec<&str> = spec
        .servers
        .iter()
        .map(|server| server.name.as_str())
        .collect();
    match spec.shape {
        Shape::Json { .. } => {
            let Ok(settings) = settings::load(&spec.path) else {
                return Vec::new();
            };
            settings
                .get("mcpServers")
                .and_then(Value::as_object)
                .map(|entries| {
                    entries
                        .keys()
                        .filter(|key| !known.contains(&key.as_str()))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        }
        Shape::TomlBlock => {
            let Ok(contents) = std::fs::read_to_string(&spec.path) else {
                return Vec::new();
            };
            let lines: Vec<&str> = contents.lines().collect();
            block_tables(&lines)
                .iter()
                .filter_map(|line| {
                    line.strip_prefix("[mcp_servers.")
                        .and_then(|rest| rest.strip_suffix(']'))
                })
                .filter(|name| !known.contains(name))
                .map(|name| name.to_string())
                .collect()
        }
    }
}

/// One declared extension's own server, named, as this client's file
/// carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpExtension {
    pub name: String,
    pub wiring: ServerWiring,
}

/// What `doctor` reads about a client's file beyond fufu's own server:
/// every declared extension's, by name, and every name registered there
/// that nothing declares any more.
pub fn extensions(spec: &Spec) -> (Vec<McpExtension>, Vec<String>) {
    let extensions = spec
        .servers
        .iter()
        .filter(|server| server.name != NAME)
        .map(|server| McpExtension {
            name: server.name.clone(),
            wiring: extension_wiring(spec, &server.name),
        })
        .collect();
    (extensions, orphaned(spec))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const EXE: &str = "/opt/fufu/bin/ff";
    const TABLE: &str = "[mcp_servers.fufu]";

    /// A declared extension's server, as `Server::extension` builds one.
    fn tower() -> Server {
        Server::extension(
            "tower",
            &crate::manifest::McpServer {
                command: "ff-tower".into(),
                args: vec!["serve".into(), "--mcp".into()],
                env: None,
            },
        )
    }

    fn json_spec(dir: &Path, with_type: bool) -> Spec {
        Spec {
            path: dir.join("mcp.json"),
            shape: Shape::Json { with_type },
            servers: vec![Server::own(EXE.into())],
        }
    }

    fn toml_spec(dir: &Path) -> Spec {
        Spec {
            path: dir.join("config.toml"),
            shape: Shape::TomlBlock,
            servers: vec![Server::own(EXE.into())],
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
    fn a_windows_binary_is_still_the_same_binary() {
        assert_eq!(binary("C:\\Users\\u\\bin\\ff.exe"), "ff");
        assert_eq!(binary("/usr/local/bin/ff"), "ff");
        assert_eq!(binary("ff"), "ff");
        assert_ne!(binary("/usr/local/bin/ffmpeg"), "ff");
        let own = Server::own(EXE.into());
        assert!(entry_is_ours(
            &own,
            &serde_json::json!({"command": "C:\\bin\\ff.exe", "args": ["mcp"]})
        ));
        assert!(!entry_is_ours(
            &own,
            &serde_json::json!({"command": "/x/ff", "args": ["serve"]})
        ));
    }

    /// An extension's server lands under its own bare name, beside fufu's,
    /// with the environment its manifest asked for; both come back out.
    #[test]
    fn an_extensions_server_lands_beside_fufus() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        let mut tower = tower();
        tower.env.insert("TOWER_BOARD".into(), "ff".into());
        spec.servers.push(tower);

        let change = install(&spec).unwrap();
        assert!(change.changed);
        assert!(
            change.lines.iter().any(|l| l.starts_with("MCP server")),
            "{:?}",
            change.lines
        );
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.starts_with("tower's MCP server registered")),
            "{:?}",
            change.lines
        );
        let after = read_json(&spec.path);
        assert_eq!(after["mcpServers"]["tower"]["type"], "stdio");
        assert_eq!(after["mcpServers"]["tower"]["command"], "ff-tower");
        assert_eq!(
            after["mcpServers"]["tower"]["args"],
            serde_json::json!(["serve", "--mcp"])
        );
        assert_eq!(after["mcpServers"]["tower"]["env"]["TOWER_BOARD"], "ff");
        assert!(!install(&spec).unwrap().changed, "idempotent");

        assert!(uninstall(&spec).unwrap().changed);
        assert_eq!(read_json(&spec.path), serde_json::json!({}));
    }

    /// A registration somebody wrote under the extension's own name is
    /// left alone, and fufu's still lands beside it.
    #[test]
    fn a_hand_written_extension_entry_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        spec.servers.push(tower());
        std::fs::write(
            &spec.path,
            r#"{"mcpServers":{"tower":{"command":"/my/wrapper.sh","args":["serve"]}}}"#,
        )
        .unwrap();

        let change = install(&spec).unwrap();
        assert!(change.changed, "fufu's own still lands");
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.contains("registers tower's MCP server by hand")),
            "{:?}",
            change.lines
        );
        let after = read_json(&spec.path);
        assert_eq!(after["mcpServers"]["tower"]["command"], "/my/wrapper.sh");
        assert!(after["mcpServers"]["fufu"].is_object());
        // fufu's own is the only wiring reported, and it is wired.
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));

        let change = uninstall(&spec).unwrap();
        assert!(change.changed);
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.contains("tower's MCP server in")),
            "{:?}",
            change.lines
        );
        let after = read_json(&spec.path);
        assert_eq!(after["mcpServers"]["tower"]["command"], "/my/wrapper.sh");
        assert!(after["mcpServers"].get("fufu").is_none());
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

    /// One marked block, a table apiece, and the whole of it removed by
    /// the markers however many tables it grew.
    #[test]
    fn the_toml_block_carries_a_table_per_server() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = toml_spec(tmp.path());
        let mut tower = tower();
        tower.env.insert("TOWER_BOARD".into(), "ff".into());
        spec.servers.push(tower);

        let change = install(&spec).unwrap();
        assert!(change.changed);
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.starts_with("tower's MCP server registered")),
            "{:?}",
            change.lines
        );
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert_eq!(
            after,
            format!(
                "{BLOCK_BEGIN}\n{TABLE}\ncommand = \"{EXE}\"\nargs = [\"mcp\"]\n\
                 [mcp_servers.tower]\ncommand = \"ff-tower\"\nargs = [\"serve\", \"--mcp\"]\n\
                 env = {{ \"TOWER_BOARD\" = \"ff\" }}\n{BLOCK_END}\n"
            )
        );
        assert!(!install(&spec).unwrap().changed, "idempotent");
        assert!(matches!(wiring(&spec), Wiring::Wired { .. }));

        let change = uninstall(&spec).unwrap();
        assert!(change.changed);
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.starts_with("tower's MCP server removed")),
            "{:?}",
            change.lines
        );
        assert_eq!(std::fs::read_to_string(&spec.path).unwrap(), "");
    }

    /// An extension dropped from the registry is written out of the block
    /// on the next install, because the block is written whole.
    #[test]
    fn a_forgotten_extension_leaves_the_toml_block() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = toml_spec(tmp.path());
        spec.servers.push(tower());
        assert!(install(&spec).unwrap().changed);
        assert!(
            std::fs::read_to_string(&spec.path)
                .unwrap()
                .contains("[mcp_servers.tower]")
        );

        spec.servers.pop();
        assert!(install(&spec).unwrap().changed);
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(!after.contains("tower"), "{after}");
        assert!(after.contains(TABLE));
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

    /// A hand-written table under an extension's name keeps fufu's block
    /// out of that name, and the block still lands for the rest.
    #[test]
    fn a_hand_written_extension_table_is_never_touched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = toml_spec(tmp.path());
        spec.servers.push(tower());
        let mine = "[mcp_servers.tower]\ncommand = \"/my/wrapper.sh\"\n";
        std::fs::write(&spec.path, mine).unwrap();

        let change = install(&spec).unwrap();
        assert!(change.changed);
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.contains("registers tower's MCP server by hand")),
            "{:?}",
            change.lines
        );
        let after = std::fs::read_to_string(&spec.path).unwrap();
        assert!(after.starts_with(mine), "{after}");
        assert!(after.contains(TABLE));
        assert_eq!(after.matches("[mcp_servers.tower]").count(), 1, "{after}");

        assert!(uninstall(&spec).unwrap().changed);
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

    // ------------------------------------------------------------------
    // what doctor reads about a declared extension's own server
    // ------------------------------------------------------------------

    /// Nothing registered under the name at all.
    #[test]
    fn an_unregistered_extension_reads_not_wired() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        spec.servers.push(tower());
        std::fs::write(&spec.path, "{}").unwrap();
        assert_eq!(extension_wiring(&spec, "tower"), ServerWiring::NotWired);
    }

    /// The entry fufu would write is wired, plainly.
    #[test]
    fn a_current_extension_entry_is_wired() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        spec.servers.push(tower());
        install(&spec).unwrap();
        assert_eq!(
            extension_wiring(&spec, "tower"),
            ServerWiring::Wired {
                at: spec.path.clone()
            }
        );
    }

    /// Same binary, different arguments: a manifest's `mcp.args` moved on
    /// without the file being rewritten. This is not hand-written — it is
    /// stale — and `install` still refuses to touch it, which is exactly
    /// why doctor must not call it fixable.
    #[test]
    fn an_entry_with_a_changed_argument_list_reads_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        std::fs::write(
            &spec.path,
            r#"{"mcpServers":{"tower":{"type":"stdio","command":"ff-tower","args":["serve"]}}}"#,
        )
        .unwrap();
        spec.servers.push(tower());
        assert_eq!(
            extension_wiring(&spec, "tower"),
            ServerWiring::Stale {
                at: spec.path.clone()
            }
        );
        // install leaves the stale entry alone — the ownership test that
        // would let it rewrite the entry is exactly the one a stale entry
        // fails — even though fufu's own still lands beside it.
        let change = install(&spec).unwrap();
        assert!(
            change
                .lines
                .iter()
                .any(|l| l.contains("registers tower's MCP server by hand")),
            "{:?}",
            change.lines
        );
        let after = read_json(&spec.path);
        assert_eq!(
            after["mcpServers"]["tower"]["args"],
            serde_json::json!(["serve"])
        );
    }

    /// A different binary entirely is hand-written, not stale.
    #[test]
    fn an_entry_running_something_else_is_hand_written() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        std::fs::write(
            &spec.path,
            r#"{"mcpServers":{"tower":{"command":"/my/wrapper.sh","args":["serve"]}}}"#,
        )
        .unwrap();
        spec.servers.push(tower());
        assert_eq!(extension_wiring(&spec, "tower"), ServerWiring::HandWritten);
    }

    /// A name nothing in `spec.servers` carries is the shape `ff extension
    /// remove` leaves behind in a JSON file: left where it sits, and named
    /// as an orphan rather than folded into the extension it no longer
    /// belongs to.
    #[test]
    fn a_json_key_nothing_declares_any_more_is_an_orphan() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        spec.servers.push(tower());
        install(&spec).unwrap();
        // tower is taken back off the registry's answer, the way
        // `ff extension remove tower` would leave `spec.servers`.
        spec.servers.pop();
        assert_eq!(orphaned(&spec), vec!["tower".to_string()]);
        // And it is no longer one of the extensions this client reports.
        let (extensions, orphaned) = extensions(&spec);
        assert!(extensions.is_empty(), "{extensions:?}");
        assert_eq!(orphaned, vec!["tower".to_string()]);
    }

    /// The same, for Codex's marked block: the orphaned table is inside
    /// fufu's own block, unambiguously fufu's to name.
    #[test]
    fn a_toml_table_nothing_declares_any_more_is_an_orphan() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = toml_spec(tmp.path());
        spec.servers.push(tower());
        install(&spec).unwrap();
        spec.servers.pop();
        assert_eq!(orphaned(&spec), vec!["tower".to_string()]);
    }

    /// `extensions` reports fufu's own server nowhere — that is `wiring`'s
    /// question — and every declared extension by name.
    #[test]
    fn extensions_never_names_fufus_own_server() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut spec = json_spec(tmp.path(), true);
        spec.servers.push(tower());
        install(&spec).unwrap();
        let (extensions, orphaned) = extensions(&spec);
        assert!(orphaned.is_empty(), "{orphaned:?}");
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "tower");
        assert_eq!(
            extensions[0].wiring,
            ServerWiring::Wired {
                at: spec.path.clone()
            }
        );
    }
}
