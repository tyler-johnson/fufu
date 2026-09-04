//! The manifest a declared extension answers with, and the `--ff-manifest`
//! handshake that asks for it.
//!
//! An undeclared extension is a filename on PATH and nothing else, which is
//! why fufu says nothing about one. Declaring is where fufu reads the
//! binary: `ff-<name> --ff-manifest` prints the manifest as an ordinary
//! envelope on one line and exits 0, so `ff extension add` can ask a binary
//! what it is before it has any reason to trust it. A manifest that does
//! not parse, names a contract fufu does not speak, or claims a name other
//! than the binary's is refused whole and nothing is recorded, because a
//! half-declared extension is one fufu would describe and could not serve.
//!
//! A manifest promising `tools` brings a second handshake with it, on the
//! same terms: `ff-<name> --ff-tools` prints the MCP tools the extension
//! produces. The list is asked for rather than written down so that it
//! comes from the definitions the extension's own CLI is built from, which
//! leaves no second spelling to keep in step. That ask is time-boxed and
//! the manifest one is not, for a reason [`ask_tools`] states.
//!
//! `docs/reference/extensions.md` types every field; this module is that
//! table in Rust.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ff_core::{Error, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::integ::event::EventKind;

/// The flag an extension recognizes before anything else on its command
/// line. It answers outside a repository and takes no other argument.
pub const FLAG: &str = "--ff-manifest";

/// The flag an extension answers with the tools it produces, on [`FLAG`]'s
/// own terms: recognized before anything else on the command line, answered
/// outside a repository, and taking no other argument.
///
/// It is the second half of the static-or-produced rule [`Briefing`] draws.
/// A manifest promises tools with `tools: true` and nothing more, and the
/// list itself is asked for here, so an extension generates it from the same
/// definitions its CLI is built from and there is no second spelling to keep
/// in step.
pub const TOOLS_FLAG: &str = "--ff-tools";

/// What a declared extension tells fufu about itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// The `<name>` in `ff-<name>`, and the namespace everything else hangs
    /// off — `cmd`, the error ids, the skills directory, the MCP server's
    /// key.
    pub name: String,
    /// The extension's own version, recorded at `add` and compared against
    /// the binary later to report drift. Opaque to fufu; nothing parses it.
    pub version: String,
    /// The machine-surface contract the extension speaks, which is the
    /// number `FF_CONTRACT` carries.
    pub contract: u32,
    /// The verbs the extension answers to, in the order it wants them
    /// listed.
    pub verbs: Vec<Verb>,
    /// Whether every write the extension makes is captured by fufu and
    /// taken back by `ff undo` — true only when it writes through fufu's
    /// own verbs.
    pub undoable: bool,
    /// One line for fufu's briefing to an agent, or `true` to ask the
    /// binary for it at print time. Absent means no line, and so does
    /// `false`, which nobody has a reason to write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub briefing: Option<Briefing>,
    /// The skill files the extension ships, absolute or relative to the
    /// directory the binary lives in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// Which neutral agent events the extension subscribes to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Subscription>,
    /// Whether the extension produces MCP tool descriptors, asked for with
    /// `ff-<name> --ff-tools`. A promise and not a list: a list recorded
    /// here would be a second spelling of the extension's own CLI, kept in
    /// step by hand and stale the moment the binary moved on. Absent is
    /// `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tools: bool,
    /// An MCP server of the extension's own, registered beside fufu's when
    /// a client is hooked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpServer>,
    /// Unknown fields are tolerated and kept, on the rule the envelope
    /// itself keeps: take fields by name. Kept rather than dropped because
    /// the registry records what was read, and a field a later contract
    /// defines must survive the round trip through a fufu that predates it.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One verb the extension answers to. Read-only is per verb rather than per
/// extension because an extension is usually mostly readers with a few
/// writers, and one set of annotations on the tool cannot say that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verb {
    pub name: String,
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// The briefing line, or the promise of one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Briefing {
    /// The line itself.
    Line(String),
    /// `true`: run `ff-<name> briefing` at print time and take its stdout.
    Ask(bool),
}

/// One subscription to the neutral agent event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    #[serde(deserialize_with = "read_kind", serialize_with = "write_kind")]
    pub kind: EventKind,
    /// The tool names this subscription wants, `|` between them and nothing
    /// else — `Edit|Write`. Required on `BeforeTool` and refused on the
    /// rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
}

impl Subscription {
    /// Whether this subscription wants an event of this kind carrying this
    /// tool name.
    ///
    /// The kind has to be the one subscribed to, and on `BeforeTool` the
    /// tool's name has to be one the matcher names. An event carrying no
    /// tool name matches nothing, so a shell prompt and a hand-taken
    /// snapshot spawn no `BeforeTool` subscriber even though the kind fufu
    /// gave them may be one.
    pub fn wants(&self, kind: EventKind, tool: Option<&str>) -> bool {
        if self.kind != kind {
            return false;
        }
        match &self.matcher {
            Some(matcher) => tool.is_some_and(|tool| names(matcher).any(|name| name == tool)),
            None => true,
        }
    }
}

/// The tool names a matcher names.
///
/// A matcher is the alternation of literal tool names every client's own
/// hook matcher already is — `Bash|Edit|Write|NotebookEdit` — and not the
/// regular expression that shape is a subset of. fufu takes no regex engine
/// for one field on one path, and the reading it does take costs no
/// compilation: a `BeforeTool` subscriber is a spawn per tool call on the
/// agent's critical path, and what stands between an event and that spawn is
/// this walk over the strings [`crate::registry::read`] already parsed.
///
/// A name matches whole and case-sensitively, against the name the client
/// spelled, so `Edit` is `Edit` and not `NotebookEdit`, and a matcher
/// written for one client's spelling does not quietly catch another's.
fn names(matcher: &str) -> impl Iterator<Item = &str> {
    matcher.split('|')
}

/// Whether every name in a matcher is one a tool could be called, which is
/// what makes the matcher one this fufu can honor.
///
/// Everything else is refused rather than read as part of a name, because a
/// person writing `Edit.*` or `^(Bash)$` means a regular expression, and a
/// matcher fufu read as the name of a tool nothing is ever called would
/// silently never fire.
fn honors(matcher: &str) -> bool {
    !matcher.is_empty() && names(matcher).all(tool_name)
}

/// Whether a string is a name a tool could be called: ASCII letters and
/// digits, `-` and `_`, and nothing else.
///
/// The characters are the ones the four clients spell tools with, MCP's
/// `mcp__server__tool` included. One rule reads both ways round — a matcher
/// names tools somebody else's client already spelled, and a descriptor
/// names one fufu is about to offer — so a name fufu would not serve is
/// also a name a subscription cannot wait for.
fn tool_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

/// The extension's own MCP server. This is where an extension wanting what
/// only a live process can hold goes — resources a client attaches and
/// re-reads, a notification when state moves, a subscription, session
/// identity across calls, a warm cache. Typed tools alone do not need one:
/// they are [`Manifest::tools`], produced by a handshake rather than spelled
/// in a manifest, and every call is a fresh process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Map<String, serde_json::Value>>,
}

/// One MCP tool a declared extension produces, as `ff-<name> --ff-tools`
/// prints it.
///
/// fufu's own type rather than `rmcp::model::Tool` deserialized straight,
/// for three reasons. This module takes serde types of its own and passes
/// none of rmcp's around, so a descriptor is checked here the way every
/// other field of the handshake is. rmcp's `Tool` carries four more fields
/// — a title, an output schema, icons, and metadata — that fufu does not
/// promise and would read and then drop. And it is `#[non_exhaustive]`, so
/// fufu could read one and could not build one, which is the wrong shape
/// for a value that has to survive being read, checked, and offered again
/// under another name.
///
/// The field spellings are MCP's, `inputSchema` and `readOnlyHint` and the
/// rest, and not the manifest's own snake case. The descriptor is MCP's
/// object; an extension that has one already copies it across, which is the
/// whole of why there is no second spelling to keep in step.
///
/// A field fufu has never heard of is tolerated and dropped, where a
/// manifest's is tolerated and kept. Nothing records a descriptor — the
/// list is asked for afresh whenever fufu needs it, and the binary is
/// always the newer of the two — so there is no round trip for an unknown
/// field to survive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    /// What the tool is called, bare. fufu namespaces it per extension
    /// before a client ever sees it, so two extensions cannot collide by
    /// both producing a `list`.
    pub name: String,
    /// What the tool does, which is the whole of what an agent reads before
    /// it calls one.
    pub description: String,
    /// The JSON Schema object a call's arguments are shaped by.
    pub input_schema: serde_json::Map<String, serde_json::Value>,
    /// What the tool says about itself. Required, where MCP leaves it out:
    /// it is the honesty that lets fufu serve the tool at all.
    pub annotations: Annotations,
}

/// What a produced tool says about itself, in MCP's own hints.
///
/// The first two are required, where MCP makes every one of them optional.
/// The one `ff` tool carries a single blanket annotation over everything it
/// relays, which is honest only when `ff undo` covers all of it; a tool that
/// states these two is honest about itself instead, and that is what a
/// produced tool is offered on. A descriptor that left them unsaid would
/// fall back to MCP's defaults — not read-only, destructive — and be a tool
/// fufu offered while knowing nothing about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    /// The tool changes nothing.
    pub read_only_hint: bool,
    /// The tool may destroy something, rather than only adding to it.
    pub destructive_hint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    /// A human-readable title, which a client may show in place of the
    /// name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// The six kinds a manifest may subscribe to, spelled in `events` exactly
/// as the event itself spells them.
///
/// [`EventKind::from_hint`] is the looser reader, and deliberately not this
/// one: it maps every vendor's spelling of a meaning onto one variant,
/// which is right for a payload fufu is translating and wrong for a field
/// somebody wrote against this page. [`EventKind::Other`] has no name here,
/// so nothing can subscribe to it.
const KINDS: [(&str, EventKind); 6] = [
    ("SessionStart", EventKind::SessionStart),
    ("ContextStart", EventKind::ContextStart),
    ("BeforeTool", EventKind::BeforeTool),
    ("SubagentStart", EventKind::SubagentStart),
    ("TurnEnd", EventKind::TurnEnd),
    ("SessionEnd", EventKind::SessionEnd),
];

fn read_kind<'de, D: Deserializer<'de>>(de: D) -> std::result::Result<EventKind, D::Error> {
    let text = String::deserialize(de)?;
    KINDS
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| {
            let names: Vec<&str> = KINDS.iter().map(|(name, _)| *name).collect();
            serde::de::Error::custom(format!(
                "unknown event kind `{text}`: one of {}",
                names.join(", ")
            ))
        })
}

/// The name a kind carries in `events`, for a caller saying back what an
/// extension subscribed to. `None` for [`EventKind::Other`], which has no
/// name here, so nothing can have subscribed to it.
pub fn kind_name(kind: EventKind) -> Option<&'static str> {
    KINDS
        .iter()
        .find(|(_, known)| *known == kind)
        .map(|(name, _)| *name)
}

fn write_kind<S: Serializer>(kind: &EventKind, se: S) -> std::result::Result<S::Ok, S::Error> {
    let name = kind_name(*kind)
        .ok_or_else(|| serde::ser::Error::custom("that event kind has no name in a manifest"))?;
    se.serialize_str(name)
}

/// What the handshake came back with: the binary fufu resolved, and what it
/// said about itself.
#[derive(Debug)]
pub struct Handshake {
    pub path: PathBuf,
    pub manifest: Manifest,
}

/// Ask `ff-<name>` on PATH what it is.
pub fn handshake(name: &str) -> Result<Handshake> {
    let Some(path) = crate::ext::resolve(name) else {
        return Err(Error::coded(
            "extension/not-found",
            format!("no ff-{name} on PATH, so there is nothing to ask for a manifest"),
            vec!["ff doctor".into()],
        ));
    };
    let manifest = ask(&path, name)?;
    Ok(Handshake { path, manifest })
}

/// The handshake against a binary already resolved: `handshake` finds one
/// on PATH, and this runs it.
///
/// Nothing is handed down. An extension needs neither the repository nor
/// the contract to say what it is, and a `FF_CONTRACT` fufu set here is a
/// number the child could read back out and echo at the check below.
/// `FF_NONINTERACTIVE` is the exception, on the rule no verb blocks on a
/// prompt with nobody there to answer.
pub fn ask(path: &Path, name: &str) -> Result<Manifest> {
    let output = Command::new(path)
        .arg(FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("FF_NONINTERACTIVE", "1")
        .output()
        .map_err(|err| failed(name, format!("it would not run: {err}")))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let said = stderr.trim();
        return Err(failed(
            name,
            if said.is_empty() {
                format!("it exited with {} and said nothing", output.status)
            } else {
                format!("it exited with {}: {said}", output.status)
            },
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(envelope) = crate::machine::one_envelope(&stdout) else {
        return Err(failed(
            name,
            "its stdout is not one envelope on one line — a banner, a progress line, or a \
             pretty-printed envelope costs it the handshake",
        ));
    };
    if let Some(error) = envelope.get("error") {
        let id = error.get("id").and_then(|id| id.as_str()).unwrap_or("");
        return Err(failed(name, format!("it answered with an error, {id}")));
    }
    let Some(data) = envelope.get("data") else {
        return Err(failed(name, "its envelope carries neither data nor error"));
    };

    let manifest = parse(data.clone())?;
    accept(&manifest, name)?;
    Ok(manifest)
}

/// Ask a declared extension for the tools it produces: `ff-<name>
/// --ff-tools`, against a binary the caller has already resolved.
///
/// Asked only of an extension whose manifest says `tools: true`. Nothing
/// here reads that field — a caller holding a manifest has already read it —
/// and a binary that promised nothing has no reason to answer.
///
/// **This asker is time-boxed and [`ask`] is not**, and the difference is
/// the caller rather than the flag. `ff extension add` and `ff doctor` are
/// verbs a person typed, watching, able to interrupt a binary that hangs.
/// The caller here is `ff mcp` starting up, where nobody is watching and a
/// hanging extension would hang a server before it ever served anything.
/// The box is [`crate::ext::BUDGET`], the same one a briefing line gets:
/// printing a list the binary already holds costs milliseconds, and a
/// second is long enough that only a binary in trouble notices it.
///
/// Nothing is handed down but `FF_NONINTERACTIVE`, exactly as [`ask`] hands
/// nothing down: an extension needs neither the repository nor the contract
/// to say what tools it has, and a `FF_CONTRACT` fufu set here is a number
/// the child could read back out and echo.
pub fn ask_tools(path: &Path, name: &str) -> Result<Vec<ToolDescriptor>> {
    let mut cmd = Command::new(path);
    cmd.arg(TOOLS_FLAG).env("FF_NONINTERACTIVE", "1");

    // stderr goes nowhere on this path, so the refusal says what fufu saw
    // rather than what the extension had to say about it. `FF_DEBUG=1` is
    // where an extension's own complaint goes everywhere else, and there is
    // no third pipe to drain here.
    let said = crate::ext::time_boxed(&mut cmd, &[], crate::ext::BUDGET)
        .map_err(|why| tools_failed(name, why))?;

    let stdout = String::from_utf8_lossy(&said);
    let Some(envelope) = crate::machine::one_envelope(&stdout) else {
        return Err(tools_failed(
            name,
            "its stdout is not one envelope on one line — a banner, a progress line, or a \
             pretty-printed envelope costs it the handshake",
        ));
    };
    if let Some(error) = envelope.get("error") {
        let id = error.get("id").and_then(|id| id.as_str()).unwrap_or("");
        return Err(tools_failed(
            name,
            format!("it answered with an error, {id}"),
        ));
    }
    let Some(data) = envelope.get("data") else {
        return Err(tools_failed(
            name,
            "its envelope carries neither data nor error",
        ));
    };
    parse_tools(data.clone())
}

/// One tool list, read and checked against the table it is typed by.
pub fn parse_tools(value: serde_json::Value) -> Result<Vec<ToolDescriptor>> {
    let tools: Vec<ToolDescriptor> =
        serde_json::from_value(value).map_err(|err| bad_tools(err.to_string()))?;
    check_tools(&tools)?;
    Ok(tools)
}

/// What a type cannot say about a tool list, on the model of the checks
/// [`check`] makes for `verbs` and `events`.
///
/// The list is refused whole rather than in part, for the reason a manifest
/// is: an extension fufu served the readable half of is one whose tools an
/// agent would call by a name that is sometimes there.
///
/// A name's length is not checked here. What a client sees is the
/// namespaced name, and how long that is belongs to whoever namespaces it.
fn check_tools(tools: &[ToolDescriptor]) -> Result<()> {
    if tools.is_empty() {
        return Err(bad_tools(
            "the list is empty: an extension that promised tools has to produce at least one",
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for tool in tools {
        if !tool_name(&tool.name) {
            return Err(bad_tools(format!(
                "`{}` is not a name a tool can be called: ASCII letters and digits, `-` and `_`, \
                 and nothing else",
                tool.name
            )));
        }
        if seen.contains(&tool.name.as_str()) {
            return Err(bad_tools(format!(
                "two tools are called `{}`, and a call names one tool",
                tool.name
            )));
        }
        seen.push(&tool.name);
        if tool.description.trim().is_empty() {
            return Err(bad_tools(format!(
                "`{}` has no description, and a description is the whole of what an agent reads \
                 before it calls",
                tool.name
            )));
        }
        if tool.input_schema.get("type").and_then(|ty| ty.as_str()) != Some("object") {
            return Err(bad_tools(format!(
                "`{}` has an inputSchema that is not `\"type\": \"object\"`, and a call's \
                 arguments arrive as an object",
                tool.name
            )));
        }
        if tool.annotations.read_only_hint && tool.annotations.destructive_hint {
            return Err(bad_tools(format!(
                "`{}` says it changes nothing and destroys something at once, and those two are \
                 what fufu serves a tool on",
                tool.name
            )));
        }
    }
    Ok(())
}

/// The handshake's own refusal: the binary ran, and what came back was not
/// a manifest fufu could get as far as parsing.
fn failed(name: &str, why: impl std::fmt::Display) -> Error {
    Error::coded(
        "extension/handshake-failed",
        format!("ff-{name} {FLAG} did not answer with a manifest: {why}"),
        vec![],
    )
}

/// One manifest, read and checked against the table it is typed by.
pub fn parse(value: serde_json::Value) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_value(value).map_err(|err| bad(err.to_string()))?;
    check(&manifest)?;
    Ok(manifest)
}

/// What a type cannot say — that `verbs` is non-empty, that a `BeforeTool`
/// subscription carries the matcher every other kind refuses.
///
/// Run on the way in from a handshake and again on the way in from the
/// registry, so a manifest a caller is holding has passed the same gate
/// whichever door it came through.
pub fn check(manifest: &Manifest) -> Result<()> {
    if !crate::ext::valid_name(&manifest.name) {
        return Err(bad(format!(
            "`{}` is not a name an extension can have: ASCII letters and digits, `-` and `_`, \
             starting with a letter or a digit",
            manifest.name
        )));
    }
    if manifest.version.is_empty() {
        return Err(bad("version is empty, so there is nothing to record"));
    }
    if manifest.verbs.is_empty() {
        return Err(bad(
            "verbs is empty: an extension fufu will describe has to answer to at least one verb",
        ));
    }
    for verb in &manifest.verbs {
        if verb.name.is_empty() || verb.name.split_whitespace().count() != 1 {
            return Err(bad(format!(
                "`{}` is not one word, and a verb's name is one word",
                verb.name
            )));
        }
    }
    for event in &manifest.events {
        match (event.kind, &event.matcher) {
            (EventKind::BeforeTool, None) => {
                return Err(bad(
                    "a BeforeTool subscription needs a matcher: every one of them is a spawn on \
                     the agent's critical path",
                ));
            }
            (EventKind::BeforeTool, Some(matcher)) => {
                if !honors(matcher) {
                    return Err(bad(format!(
                        "`{matcher}` is not a matcher fufu can honor: matcher is the tool names \
                         the subscription wants with `|` between them, like `Edit|Write`, and \
                         not a regular expression"
                    )));
                }
            }
            (_, Some(_)) => {
                return Err(bad(
                    "matcher is the tool names a subscription wants, and only BeforeTool carries \
                     one",
                ));
            }
            (_, None) => {}
        }
    }
    if let Some(mcp) = &manifest.mcp
        && mcp.command.is_empty()
    {
        return Err(bad("mcp.command is empty, so there is no server to run"));
    }
    Ok(())
}

fn bad(why: impl std::fmt::Display) -> Error {
    Error::coded(
        "extension/bad-manifest",
        format!("that is not a manifest fufu can read: {why}"),
        vec![],
    )
}

/// The tool handshake's own refusal, which is [`failed`]'s twin: the binary
/// ran, and what came back was not a tool list fufu could get as far as
/// parsing. A separate id from the manifest handshake's, because there are
/// two handshakes now and a reader has to be able to tell which of them the
/// extension fell down on.
fn tools_failed(name: &str, why: impl std::fmt::Display) -> Error {
    Error::coded(
        "extension/tools-failed",
        format!("ff-{name} {TOOLS_FLAG} did not answer with a tool list: {why}"),
        vec!["ff doctor".into()],
    )
}

fn bad_tools(why: impl std::fmt::Display) -> Error {
    Error::coded(
        "extension/bad-tools",
        format!("that is not a tool list fufu can read: {why}"),
        vec![],
    )
}

/// The two checks that are about this fufu and this binary rather than
/// about the manifest's own shape, in the order the contract states them.
fn accept(manifest: &Manifest, name: &str) -> Result<()> {
    if manifest.contract != crate::machine::CONTRACT {
        return Err(Error::coded(
            "extension/unsupported-contract",
            format!(
                "ff-{name} speaks contract {}, and this fufu speaks {}",
                manifest.contract,
                crate::machine::CONTRACT
            ),
            vec![],
        ));
    }
    if manifest.name != name {
        return Err(Error::coded(
            "extension/name-mismatch",
            format!(
                "ff-{name} calls itself `{}`, and a manifest names the binary fufu resolved",
                manifest.name
            ),
            vec![],
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked payload from `docs/reference/extensions.md`, with every
    /// optional field present.
    const WORKED: &str = r#"{
        "name": "tower",
        "version": "0.4.1",
        "contract": 1,
        "verbs": [
            {"name": "board", "read_only": true, "summary": "what is filed"},
            {"name": "done", "read_only": false}
        ],
        "undoable": true,
        "briefing": "Work is filed as flights on a board.",
        "skills": ["/usr/local/share/tower/skills/tower.md"],
        "events": [{"kind": "SessionStart"}, {"kind": "BeforeTool", "matcher": "Edit|Write"}],
        "tools": true,
        "mcp": {"command": "ff", "args": ["tower", "serve", "--mcp"]}
    }"#;

    /// The worked tool list from the same page, which is what `--ff-tools`
    /// answers with.
    const TOOLS: &str = r#"[
        {
            "name": "board",
            "description": "What is filed, what is moving, and what is stuck.",
            "inputSchema": {
                "type": "object",
                "properties": {"branch": {"type": "string"}},
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false}
        },
        {
            "name": "file",
            "description": "File a flight on the board.",
            "inputSchema": {
                "type": "object",
                "properties": {"title": {"type": "string"}},
                "required": ["title"]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false,
                "title": "File a flight"
            }
        }
    ]"#;

    fn value(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("valid json")
    }

    #[test]
    fn the_worked_payload_parses_field_for_field() {
        let manifest = parse(value(WORKED)).expect("the page's own manifest");
        assert_eq!(manifest.name, "tower");
        assert_eq!(manifest.version, "0.4.1");
        assert_eq!(manifest.contract, 1);
        assert!(manifest.undoable);
        assert_eq!(manifest.verbs.len(), 2);
        assert_eq!(manifest.verbs[0].name, "board");
        assert!(manifest.verbs[0].read_only);
        assert_eq!(manifest.verbs[0].summary.as_deref(), Some("what is filed"));
        assert!(!manifest.verbs[1].read_only);
        assert_eq!(manifest.verbs[1].summary, None);
        assert!(matches!(manifest.briefing, Some(Briefing::Line(_))));
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.events[0].kind, EventKind::SessionStart);
        assert_eq!(manifest.events[1].kind, EventKind::BeforeTool);
        assert_eq!(manifest.events[1].matcher.as_deref(), Some("Edit|Write"));
        assert!(manifest.tools, "a promise, and the list is asked for");
        let mcp = manifest.mcp.expect("mcp");
        assert_eq!(mcp.command, "ff");
        assert_eq!(mcp.args, ["tower", "serve", "--mcp"]);
    }

    /// The five optional fields are optional, and absent is not empty by a
    /// different spelling.
    #[test]
    fn the_smallest_manifest_parses() {
        let manifest = parse(value(
            r#"{"name":"tower","version":"0.1.0","contract":1,
                "verbs":[{"name":"board","read_only":true}],"undoable":false}"#,
        ))
        .expect("the required five");
        assert!(manifest.briefing.is_none());
        assert!(manifest.skills.is_empty());
        assert!(manifest.events.is_empty());
        assert!(!manifest.tools);
        assert!(manifest.mcp.is_none());
        assert!(manifest.extra.is_empty());
    }

    /// `tools` is a promise and nothing more, and a manifest that promised
    /// nothing says nothing: `false` is not written back out, so a record
    /// made by a fufu that predates the field reads and writes identically.
    #[test]
    fn tools_is_a_promise_that_absent_declines() {
        let promised = parse(value(WORKED)).expect("the page's own manifest");
        assert!(promised.tools);
        assert_eq!(
            serde_json::to_value(&promised).expect("serialize")["tools"],
            serde_json::Value::Bool(true)
        );

        let silent = parse(value(
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}]}"#,
        ))
        .expect("no promise");
        assert!(!silent.tools);
        let written = serde_json::to_value(&silent).expect("serialize");
        assert!(written.get("tools").is_none(), "{written}");
    }

    #[test]
    fn a_required_field_missing_is_refused() {
        for text in [
            r#"{"version":"1","contract":1,"verbs":[{"name":"b","read_only":true}],"undoable":true}"#,
            r#"{"name":"tower","contract":1,"verbs":[{"name":"b","read_only":true}],"undoable":true}"#,
            r#"{"name":"tower","version":"1","verbs":[{"name":"b","read_only":true}],"undoable":true}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true}"#,
            r#"{"name":"tower","version":"1","contract":1,"verbs":[{"name":"b","read_only":true}]}"#,
        ] {
            let err = parse(value(text)).expect_err(text);
            assert_eq!(err.id(), "extension/bad-manifest", "{text}");
        }
    }

    /// `briefing` is a string or `true`, and the untagged read has to keep
    /// the two apart rather than stringifying one into the other.
    #[test]
    fn briefing_is_a_line_or_a_promise_of_one() {
        let asked = parse(value(
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"briefing":true}"#,
        ))
        .expect("briefing true");
        assert!(matches!(asked.briefing, Some(Briefing::Ask(true))));
    }

    /// Take fields by name: a field fufu has never heard of is kept, so a
    /// manifest recorded by this fufu and read by a later one is the
    /// manifest the extension printed.
    #[test]
    fn an_unknown_field_survives_the_round_trip() {
        let manifest = parse(value(
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"colors":{"badge":"amber"}}"#,
        ))
        .expect("an unknown field is tolerated");
        assert_eq!(manifest.extra["colors"]["badge"], "amber");
        let round_tripped = serde_json::to_value(&manifest).expect("serialize");
        assert_eq!(round_tripped["colors"]["badge"], "amber");
        assert_eq!(round_tripped["name"], "tower");
    }

    #[test]
    fn a_manifest_that_does_not_hold_together_is_refused() {
        for text in [
            // A name the dispatcher would never have resolved.
            r#"{"name":"../evil","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}]}"#,
            r#"{"name":"tower","version":"","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,"verbs":[]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"bay warm","read_only":true}]}"#,
            // A kind no event has, and a vendor's spelling of one that does.
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"events":[{"kind":"Other"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"events":[{"kind":"PreToolUse"}]}"#,
            // The matcher every BeforeTool needs, and no other kind takes.
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"events":[{"kind":"BeforeTool"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"TurnEnd","matcher":"Edit"}]}"#,
            // A matcher fufu cannot honor: a regular expression, an empty
            // alternative, and nothing at all.
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"BeforeTool","matcher":"Edit.*"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"BeforeTool","matcher":"^(Bash|Edit)$"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"BeforeTool","matcher":"Edit|"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"BeforeTool","matcher":""}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],
                "events":[{"kind":"BeforeTool","matcher":"*"}]}"#,
            r#"{"name":"tower","version":"1","contract":1,"undoable":true,
                "verbs":[{"name":"b","read_only":true}],"mcp":{"command":""}}"#,
        ] {
            let err = parse(value(text)).expect_err(text);
            assert_eq!(err.id(), "extension/bad-manifest", "{text}");
        }
    }

    /// The matchers a subscription can be written with: one tool name, the
    /// alternation of several, and the punctuation the four clients spell
    /// tools with.
    #[test]
    fn a_matcher_is_the_tool_names_it_wants() {
        for matcher in [
            "Edit",
            "Bash|Edit|Write|NotebookEdit",
            "run_shell_command|write_file|replace",
            "apply_patch",
            "mcp__plugin_fufu_fufu__ff",
        ] {
            let text = format!(
                r#"{{"name":"tower","version":"1","contract":1,"undoable":true,
                    "verbs":[{{"name":"b","read_only":true}}],
                    "events":[{{"kind":"BeforeTool","matcher":"{matcher}"}}]}}"#
            );
            parse(value(&text)).unwrap_or_else(|err| panic!("{matcher}: {err}"));
        }
    }

    /// A subscription wants an event when the kind is the one it named and
    /// the matcher names the tool. A name matches whole and by case, and an
    /// event carrying no tool name matches nothing, so a shell prompt spawns
    /// no `BeforeTool` subscriber.
    #[test]
    fn a_subscription_wants_the_kind_and_the_tool_it_named() {
        let sub = Subscription {
            kind: EventKind::BeforeTool,
            matcher: Some("Edit|Write".into()),
        };
        assert!(sub.wants(EventKind::BeforeTool, Some("Edit")));
        assert!(sub.wants(EventKind::BeforeTool, Some("Write")));
        assert!(!sub.wants(EventKind::BeforeTool, Some("Bash")));
        assert!(!sub.wants(EventKind::BeforeTool, Some("NotebookEdit")));
        assert!(!sub.wants(EventKind::BeforeTool, Some("edit")));
        assert!(!sub.wants(EventKind::BeforeTool, Some("Edit|Write")));
        assert!(!sub.wants(EventKind::BeforeTool, None));
        assert!(!sub.wants(EventKind::TurnEnd, Some("Edit")));

        // Every other kind carries no matcher, and the kind is the whole of
        // the test there.
        let sub = Subscription {
            kind: EventKind::TurnEnd,
            matcher: None,
        };
        assert!(sub.wants(EventKind::TurnEnd, None));
        assert!(!sub.wants(EventKind::SessionEnd, None));
    }

    #[test]
    fn the_worked_tool_list_parses_field_for_field() {
        let tools = parse_tools(value(TOOLS)).expect("the page's own tool list");
        assert_eq!(tools.len(), 2);

        assert_eq!(tools[0].name, "board");
        assert!(tools[0].description.starts_with("What is filed"));
        assert_eq!(tools[0].input_schema["type"], "object");
        assert!(tools[0].input_schema["properties"]["branch"].is_object());
        assert!(tools[0].annotations.read_only_hint);
        assert!(!tools[0].annotations.destructive_hint);
        // The two MCP leaves out are left out, and absent is not false by a
        // different spelling.
        assert_eq!(tools[0].annotations.idempotent_hint, None);
        assert_eq!(tools[0].annotations.open_world_hint, None);
        assert_eq!(tools[0].annotations.title, None);

        assert_eq!(tools[1].name, "file");
        assert!(!tools[1].annotations.read_only_hint);
        assert_eq!(tools[1].annotations.idempotent_hint, Some(false));
        assert_eq!(tools[1].annotations.open_world_hint, Some(false));
        assert_eq!(tools[1].annotations.title.as_deref(), Some("File a flight"));
    }

    /// The descriptor is written in MCP's spellings, so an extension that
    /// already has one copies it across rather than translating it.
    #[test]
    fn a_descriptor_is_spelled_the_way_mcp_spells_one() {
        let tools = parse_tools(value(TOOLS)).expect("parses");
        let written = serde_json::to_value(&tools[1]).expect("serialize");
        assert!(written["inputSchema"].is_object());
        assert_eq!(written["annotations"]["readOnlyHint"], false);
        assert_eq!(written["annotations"]["destructiveHint"], false);
        assert_eq!(written["annotations"]["openWorldHint"], false);
        assert!(
            written.get("input_schema").is_none(),
            "the manifest's snake case is not the descriptor's: {written}"
        );

        // Read back into the type the relay will hand a client, since the
        // page promises these are the same four fields under both names.
        let tool: rmcp::model::Tool = serde_json::from_value(written).expect("an rmcp tool");
        assert_eq!(tool.name, "file");
        assert_eq!(
            tool.annotations.and_then(|a| a.destructive_hint),
            Some(false)
        );
    }

    /// A field fufu has never heard of is read past rather than refused, and
    /// dropped rather than kept: nothing records a descriptor, so there is
    /// no round trip for it to survive.
    #[test]
    fn an_unknown_field_on_a_descriptor_is_read_past() {
        let tools = parse_tools(value(
            r#"[{"name":"board","description":"the board","outputSchema":{"type":"object"},
                 "inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false,"colors":"amber"}}]"#,
        ))
        .expect("an unknown field is tolerated");
        let written = serde_json::to_value(&tools[0]).expect("serialize");
        assert!(written.get("outputSchema").is_none(), "{written}");
        assert!(written["annotations"].get("colors").is_none(), "{written}");
    }

    #[test]
    fn a_tool_list_that_does_not_hold_together_is_refused() {
        for text in [
            // An extension that promised tools produced none.
            "[]",
            // Not a list at all.
            r#"{"name":"board","description":"d","inputSchema":{"type":"object"},
                "annotations":{"readOnlyHint":true,"destructiveHint":false}}"#,
            // A name nothing could be called, and one no client could spell.
            r#"[{"name":"","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"the board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"tower.board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            // Two tools of one name: a call names one tool.
            r#"[{"name":"board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}},
                {"name":"board","description":"e","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":false,"destructiveHint":false}}]"#,
            // Nothing for an agent to read before it calls.
            r#"[{"name":"board","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"board","description":"  ","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            // A schema that is not there, is not an object, and describes
            // something a call's arguments could never be.
            r#"[{"name":"board","description":"d",
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"board","description":"d","inputSchema":"object",
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"board","description":"d","inputSchema":{},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            r#"[{"name":"board","description":"d","inputSchema":{"type":"array"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":false}}]"#,
            // The two hints fufu requires and MCP does not.
            r#"[{"name":"board","description":"d","inputSchema":{"type":"object"}}]"#,
            r#"[{"name":"board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true}}]"#,
            r#"[{"name":"board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"destructiveHint":false}}]"#,
            // Both at once, which says two things.
            r#"[{"name":"board","description":"d","inputSchema":{"type":"object"},
                 "annotations":{"readOnlyHint":true,"destructiveHint":true}}]"#,
        ] {
            let err = parse_tools(value(text)).expect_err(text);
            assert_eq!(err.id(), "extension/bad-tools", "{text}");
        }
    }

    /// The contract check is what the handshake exists for, and it runs
    /// before the name check on the order the contract states them.
    #[test]
    fn a_contract_this_fufu_does_not_speak_is_refused() {
        let manifest = parse(value(
            &WORKED.replace("\"contract\": 1", "\"contract\": 99"),
        ))
        .expect("parses, and is refused later");
        let err = accept(&manifest, "tower").expect_err("contract 99");
        assert_eq!(err.id(), "extension/unsupported-contract");
        assert!(err.to_string().contains("99"), "{err}");
    }

    #[test]
    fn a_manifest_claiming_another_binarys_name_is_refused() {
        let manifest = parse(value(WORKED)).expect("parses");
        let err = accept(&manifest, "bay").expect_err("tower is not bay");
        assert_eq!(err.id(), "extension/name-mismatch");
        assert!(accept(&manifest, "tower").is_ok());
    }

    #[test]
    fn the_contract_matches_the_one_the_extension_was_handed() {
        assert_eq!(
            parse(value(WORKED)).expect("parses").contract,
            crate::machine::CONTRACT
        );
    }

    /// The handshake runs a real binary, and a shell script is the smallest
    /// one to write. Unix only, for the reason `tests/ext.rs` is.
    #[cfg(unix)]
    mod against_a_binary {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        /// One envelope on one line, which is the whole of what the
        /// handshake reads. The manifests above are written for the eye, so
        /// the data is compacted on the way in rather than echoed as it is
        /// spelled.
        fn envelope(data: &str) -> String {
            let data = serde_json::to_string(&value(data)).expect("compact");
            format!(r#"{{"ff":1,"cmd":"tower --ff-manifest","data":{data}}}"#)
        }

        /// The same, for the other handshake.
        fn tools_envelope(data: &str) -> String {
            let data = serde_json::to_string(&value(data)).expect("compact");
            format!(r#"{{"ff":1,"cmd":"tower --ff-tools","data":{data}}}"#)
        }

        /// An `ff-<name>` in a fresh directory the caller keeps alive.
        fn ext_bin(name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let path = dir.path().join(format!("ff-{name}"));
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
            std::fs::set_permissions(&path, Permissions::from_mode(0o755)).expect("chmod script");
            (dir, path)
        }

        fn asking(body: &str) -> Result<Manifest> {
            let (_dir, path) = ext_bin("tower", body);
            ask(&path, "tower")
        }

        fn asking_tools(body: &str) -> Result<Vec<ToolDescriptor>> {
            let (_dir, path) = ext_bin("tower", body);
            ask_tools(&path, "tower")
        }

        #[test]
        fn one_envelope_on_one_line_is_the_manifest() {
            let manifest = asking(&format!("echo '{}'", envelope(WORKED))).expect("handshake");
            assert_eq!(manifest.name, "tower");
            assert_eq!(manifest.verbs.len(), 2);
        }

        /// The flag is the whole command line, so a binary that answers
        /// something else was asked something else.
        #[test]
        fn the_flag_is_the_only_argument() {
            let manifest = asking(&format!(
                "test \"$1\" = '--ff-manifest' && test $# -eq 1 && echo '{}'",
                envelope(WORKED)
            ))
            .expect("handshake");
            assert_eq!(manifest.name, "tower");
        }

        #[test]
        fn a_binary_that_fails_the_handshake_is_refused() {
            for body in [
                // Exited nonzero, with something to say and without.
                "echo 'no manifest here' >&2; exit 1",
                "exit 3",
                // Exited 0, printing something that is not an envelope.
                "echo hello",
                "echo",
                // Two envelopes, and one pretty-printed over several lines.
                &format!("echo '{}'; echo '{}'", envelope(WORKED), envelope(WORKED)),
                &format!("echo '{{\"ff\":1,\"cmd\":\"tower --ff-manifest\",\"data\":{WORKED}}}'"),
                // A banner before the envelope.
                &format!("echo tower 0.4.1; echo '{}'", envelope(WORKED)),
                // An envelope carrying an error, and one carrying neither.
                r#"echo '{"ff":1,"cmd":"tower --ff-manifest","error":{"id":"tower/usage/no-such-flag","message":"no","exits":[]}}'"#,
                r#"echo '{"ff":1,"cmd":"tower --ff-manifest"}'"#,
                // JSON, but not fufu's envelope.
                r#"echo '{"tower":1,"data":{}}'"#,
            ] {
                let err = asking(body).expect_err(body);
                assert_eq!(err.id(), "extension/handshake-failed", "{body}");
            }
        }

        /// A binary that answers is still refused on what it answered, and
        /// the refusal names which check it failed.
        #[test]
        fn the_checks_run_on_what_the_binary_said() {
            let err = asking(&format!("echo '{}'", envelope(r#"{"name":"tower"}"#)))
                .expect_err("half a manifest");
            assert_eq!(err.id(), "extension/bad-manifest");

            let bad_contract = WORKED.replace("\"contract\": 1", "\"contract\": 99");
            let err =
                asking(&format!("echo '{}'", envelope(&bad_contract))).expect_err("contract 99");
            assert_eq!(err.id(), "extension/unsupported-contract");

            let other = WORKED.replace("\"name\": \"tower\"", "\"name\": \"bay\"");
            let err = asking(&format!("echo '{}'", envelope(&other))).expect_err("not tower");
            assert_eq!(err.id(), "extension/name-mismatch");
        }

        /// Nothing on stdin, so a binary that reads it is not left waiting
        /// on a person who is not there.
        #[test]
        fn stdin_is_closed() {
            let manifest =
                asking(&format!("cat >/dev/null; echo '{}'", envelope(WORKED))).expect("handshake");
            assert_eq!(manifest.name, "tower");
        }

        /// `handshake` is `ask` with the PATH walk in front, and the walk
        /// answers for a name nothing on PATH spells.
        #[test]
        fn a_name_with_no_binary_is_refused_before_anything_runs() {
            let err = handshake("nothing-on-path-answers-to-this").expect_err("no such binary");
            assert_eq!(err.id(), "extension/not-found");
        }

        #[test]
        fn one_envelope_on_one_line_is_the_tool_list() {
            let tools =
                asking_tools(&format!("echo '{}'", tools_envelope(TOOLS))).expect("handshake");
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].name, "board");
        }

        /// The flag is the whole command line here too, and nothing is
        /// handed down but `FF_NONINTERACTIVE`: an extension needs neither
        /// the repository nor the contract to say what tools it has.
        #[test]
        fn the_tools_flag_is_the_only_argument() {
            let tools = asking_tools(&format!(
                "test \"$1\" = '--ff-tools' && test $# -eq 1 && test \"$FF_NONINTERACTIVE\" = 1 \
                 && echo '{}'",
                tools_envelope(TOOLS)
            ))
            .expect("handshake");
            assert_eq!(tools.len(), 2);
        }

        /// Nothing on stdin, so a binary that reads it is not left waiting.
        #[test]
        fn the_tools_handshake_closes_stdin() {
            let tools = asking_tools(&format!("cat >/dev/null; echo '{}'", tools_envelope(TOOLS)))
                .expect("handshake");
            assert_eq!(tools.len(), 2);
        }

        #[test]
        fn a_binary_that_fails_the_tools_handshake_is_refused() {
            for body in [
                "echo 'no tools here' >&2; exit 1",
                "exit 3",
                "echo hello",
                "echo",
                &format!(
                    "echo '{}'; echo '{}'",
                    tools_envelope(TOOLS),
                    tools_envelope(TOOLS)
                ),
                &format!("echo tower 0.4.1; echo '{}'", tools_envelope(TOOLS)),
                r#"echo '{"ff":1,"cmd":"tower --ff-tools","error":{"id":"tower/usage/no-such-flag","message":"no","exits":[]}}'"#,
                r#"echo '{"ff":1,"cmd":"tower --ff-tools"}'"#,
                r#"echo '{"tower":1,"data":[]}'"#,
            ] {
                let err = asking_tools(body).expect_err(body);
                assert_eq!(err.id(), "extension/tools-failed", "{body}");
            }
        }

        /// A binary that answered is still refused on what it answered.
        #[test]
        fn the_tool_checks_run_on_what_the_binary_said() {
            let err = asking_tools(&format!("echo '{}'", tools_envelope("[]")))
                .expect_err("promised tools and produced none");
            assert_eq!(err.id(), "extension/bad-tools");
        }

        /// The landmine this asker exists for. `ask` waits as long as a
        /// person is willing to; this one is boxed, because its caller is a
        /// server starting up with nobody there to interrupt it. Both
        /// shapes of hang are covered: a binary still thinking when the
        /// budget runs out, and one that exited leaving a grandchild
        /// holding the write end of the pipe — a live process the box can
        /// see, and a dead one whose pipe it cannot.
        #[test]
        fn a_binary_that_hangs_costs_the_tools_handshake_the_budget() {
            // PATH is the script's own directory for the length of the ask,
            // so it names a system one to reach `sleep`.
            let hang = "PATH=/bin:/usr/bin; export PATH; sleep 120";
            let started = std::time::Instant::now();
            let err = asking_tools(hang).expect_err("it never answered");
            let waited = started.elapsed();
            assert_eq!(err.id(), "extension/tools-failed");
            assert!(
                waited >= crate::ext::BUDGET && waited < crate::ext::BUDGET * 10,
                "waited {waited:?} against a budget of {:?}",
                crate::ext::BUDGET
            );

            let orphan = "PATH=/bin:/usr/bin; export PATH; sleep 120 & exit 0";
            let started = std::time::Instant::now();
            let err = asking_tools(orphan).expect_err("its stdout never closed");
            let waited = started.elapsed();
            assert_eq!(err.id(), "extension/tools-failed");
            assert!(
                waited < crate::ext::BUDGET * 10,
                "waited {waited:?} against a budget of {:?}",
                crate::ext::BUDGET
            );
        }
    }
}
