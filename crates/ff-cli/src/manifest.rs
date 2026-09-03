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
//! `docs/agents/machine-surface.md` types every field; this module is that
//! table in Rust.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ff_core::{Error, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::integ::event::EventKind;

/// The flag an extension recognizes before anything else on its command
/// line. It answers outside a repository and takes no other argument.
pub const FLAG: &str = "--ff-manifest";

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
/// The characters are the ones the four clients spell tools with, MCP's
/// `mcp__server__tool` included. Everything else is refused rather than read
/// as part of a name, because a person writing `Edit.*` or `^(Bash)$` means
/// a regular expression, and a matcher fufu read as the name of a tool
/// nothing is ever called would silently never fire.
fn honors(matcher: &str) -> bool {
    !matcher.is_empty()
        && names(matcher).all(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        })
}

/// The extension's own MCP server. This is where an extension wanting its
/// own tools, annotations, or resources goes; fufu never proxies typed
/// tools out of a manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Map<String, serde_json::Value>>,
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

    /// One envelope on one line, which is the whole of what the handshake
    /// reads. The manifests below are written for the eye, so the data is
    /// compacted on the way in rather than echoed as it is spelled.
    fn envelope(data: &str) -> String {
        let data = serde_json::to_string(&value(data)).expect("compact");
        format!(r#"{{"ff":1,"cmd":"tower --ff-manifest","data":{data}}}"#)
    }

    /// The worked payload from `docs/agents/machine-surface.md`, with every
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
        "mcp": {"command": "ff", "args": ["tower", "serve", "--mcp"]}
    }"#;

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
        let mcp = manifest.mcp.expect("mcp");
        assert_eq!(mcp.command, "ff");
        assert_eq!(mcp.args, ["tower", "serve", "--mcp"]);
    }

    /// The four optional fields are optional, and absent is not empty by a
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
        assert!(manifest.mcp.is_none());
        assert!(manifest.extra.is_empty());
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
    }
}
