//! The tools a declared extension produced, listed beside fufu's own.
//!
//! An extension whose manifest promises `tools` is asked `ff-<name>
//! --ff-tools` once, when the server starts, and what it answered is what
//! the connection serves until it closes. That is how `registry::read` is
//! held too: the verbs advertised at handshake are the verbs served for the
//! life of the connection, so the card, the relay, and this list cannot
//! disagree with each other halfway through. A restart is what picks up an
//! edited extension.
//!
//! A handshake that fails or never answers is silence, on `ff trigger`'s
//! doctrine: fufu serves its own tool, the extension's verbs are relayed in
//! the args array exactly as they were, and what is lost is the tools it
//! promised. `ff doctor` is where that shows. The ask is time-boxed for the
//! reason [`crate::manifest::ask_tools`] states — nobody is in front of a
//! server starting up, and a binary that hangs would hang it before it ever
//! served anything.
//!
//! A tool is named `<extension>__<tool>`, which is the shape MCP itself
//! uses when a client prefixes a server's tools, so two extensions both
//! producing a `list` cannot collide and a name routes back to a binary and
//! a verb by reading it. The bare `ff` tool keeps its own name: it is
//! spelled into the `fufu.toolPolicy` refusal, and an agent told to call it
//! must not have to look it up.
//!
//! A call is routed the way an args-array call is — this same binary as a
//! child, `<exe> <name> <verb> <words…> --json` — because that is what
//! keeps capture-first, the git policy, sessions, and error ids true of a
//! produced tool's call. Spawning `ff-<name>` here would be a second spawn
//! path with a second set of promises to keep. What is new is only the
//! words: the args array arrives as words already, and a produced tool
//! arrives as an object, so [`Produced::call`] is where an object becomes
//! a command line.

use rmcp::ErrorData;
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::Value;

use crate::manifest::ToolDescriptor;
use crate::registry::Registry;

use super::child::{self, Call, Route};
use super::describe;

/// What stands between the extension and the tool in the name a client
/// calls. MCP's own separator, and one `manifest::honors` already tolerates
/// in an event matcher.
const SEPARATOR: &str = "__";

/// The one keyword fufu reads out of a descriptor's `inputSchema` beyond
/// JSON Schema's own: an array of property names, spelled as bare words in
/// that array's order before every option.
///
/// A command line is not an object, and most CLI verbs take a positional —
/// `ff tower brief 98` — so without this an extension would have to grow a
/// flag it does not otherwise have, which is the second spelling the whole
/// handshake exists to avoid. JSON Schema tolerates a keyword it has never
/// heard of, so the schema goes to the client as it arrived.
const POSITIONAL: &str = "positional";

/// One tool a declared extension produced, as this server serves it.
pub struct Produced {
    /// What a client calls it: `<extension>__<tool>`.
    name: String,
    /// The extension the call routes to, and the verb under it. The bare
    /// name a descriptor carries *is* the verb — `cmd` is spelled `<name>
    /// <verb>` for a produced tool's call the same as for a relayed one.
    extension: String,
    verb: String,
    /// The properties spelled as bare words, in the order [`POSITIONAL`]
    /// listed them.
    positional: Vec<String>,
    /// The tool as the client is offered it.
    tool: Tool,
}

impl Produced {
    /// The name a client calls this by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The tool as the client lists it.
    pub fn tool(&self) -> Tool {
        self.tool.clone()
    }

    /// The arguments object as a command line, or the reason it is not one.
    ///
    /// `Err` is a JSON-RPC error rather than an envelope, on the rule
    /// [`child::parse`] keeps: nothing ran, there is no envelope to hand
    /// over, and what is wrong is the shape of the arguments.
    ///
    /// Every property is spelled the way a person would spell it: a
    /// positional as its value alone, a true boolean as `--key` and a false
    /// one as nothing at all, anything else as `--key <value>`, and an
    /// array by repeating its flag. The property is spelled verbatim, with
    /// no case or underscore translation, because the extension generates
    /// the schema from the same definitions its own flags come from.
    pub fn call(&self, arguments: Option<JsonObject>) -> Result<Call, ErrorData> {
        let arguments = arguments.unwrap_or_default();
        let mut args = vec![self.extension.clone(), self.verb.clone()];
        for key in &self.positional {
            // A gap stops the rest: a positional left out shifts every word
            // after it onto the wrong argument, and a line the extension
            // misreads is worse than one it refuses.
            match arguments.get(key.as_str()) {
                None | Some(Value::Null) => break,
                Some(Value::Array(items)) => {
                    for item in items {
                        args.push(word(key, item)?);
                    }
                }
                Some(value) => args.push(word(key, value)?),
            }
        }
        // Read rather than drained, so what is left keeps the order the
        // client sent it in, and a positional is never spelled twice.
        for (key, value) in &arguments {
            if self.positional.contains(key) {
                continue;
            }
            if !spellable(key) {
                return Err(ErrorData::invalid_params(
                    format!("`{key}` is not a name a command line has an option for"),
                    None,
                ));
            }
            args.extend(flag(key, value)?);
        }
        Ok(Call {
            args,
            cwd: None,
            route: Route::Produced,
        })
    }
}

/// The tools this server serves beside its own, asked for once.
///
/// Silent throughout: an extension that promised tools and would not answer
/// costs the agent nothing and is not reported here.
///
/// Every extension that promised tools is asked, undoable or not. What
/// makes a produced tool honest is the `readOnlyHint` and `destructiveHint`
/// the descriptor carries, not the one `ff` tool's blanket promise, so
/// `undoable: false` bars the args array and nothing else — `child::Route`
/// is where that line is drawn, and `toolpolicy::served` is where the shell
/// reads it back.
pub fn produced(registry: &Registry) -> Vec<Produced> {
    let mut out = Vec::new();
    for entry in registry.declared() {
        if !entry.manifest.tools {
            continue;
        }
        // A verb the relay refuses is a verb no produced tool could route
        // to either, so its tools are never offered.
        if child::EXCLUDED.contains(&entry.name()) {
            continue;
        }
        let Some(path) = entry.resolve() else {
            continue;
        };
        let Ok(descriptors) = crate::manifest::ask_tools(&path, entry.name()) else {
            continue;
        };
        offer(&mut out, entry.name(), descriptors);
    }
    out
}

/// One extension's descriptors, namespaced and folded into the list.
///
/// A name already taken is dropped and the rest of the list stands. Two
/// extensions cannot produce one name — the extension is in it — but an
/// extension name may itself carry `_`, so `a__b` and `a` producing `b__c`
/// can meet. The first declared keeps the name, which is the order
/// `ff extension list` prints and the order a person can read. `ff` itself
/// is never taken — the separator is in every produced name, and the check
/// below is where that rule is written down rather than assumed.
fn offer(out: &mut Vec<Produced>, extension: &str, descriptors: Vec<ToolDescriptor>) {
    for descriptor in descriptors {
        let name = format!("{extension}{SEPARATOR}{}", descriptor.name);
        if name == describe::NAME || out.iter().any(|produced| produced.name == name) {
            continue;
        }
        let positional = positional(&descriptor.input_schema);
        let tool = Tool::new(
            name.clone(),
            descriptor.description,
            descriptor.input_schema,
        )
        .with_annotations(annotations(&descriptor.annotations));
        out.push(Produced {
            name,
            extension: extension.to_string(),
            verb: descriptor.name,
            positional,
            tool,
        });
    }
}

/// The properties [`POSITIONAL`] names, and nothing when the schema names
/// none or spells the keyword as something other than an array of strings.
fn positional(schema: &serde_json::Map<String, Value>) -> Vec<String> {
    schema
        .get(POSITIONAL)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The descriptor's hints as the client's. The two fufu requires are
/// always said; the three MCP leaves optional are said only when the
/// extension said them, so an unstated hint reaches the client unstated.
fn annotations(said: &crate::manifest::Annotations) -> ToolAnnotations {
    let mut annotations = match &said.title {
        Some(title) => ToolAnnotations::with_title(title.clone()),
        None => ToolAnnotations::new(),
    };
    annotations = annotations
        .read_only(said.read_only_hint)
        .destructive(said.destructive_hint);
    if let Some(idempotent) = said.idempotent_hint {
        annotations = annotations.idempotent(idempotent);
    }
    if let Some(open_world) = said.open_world_hint {
        annotations = annotations.open_world(open_world);
    }
    annotations
}

/// Whether a property name is one a command line has an option for.
fn spellable(key: &str) -> bool {
    !key.is_empty()
        && key.chars().next().is_some_and(char::is_alphanumeric)
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

/// One property as the words it is spelled as.
fn flag(key: &str, value: &Value) -> Result<Vec<String>, ErrorData> {
    match value {
        // Absent and false are the same thing on a command line: the flag
        // is not there.
        Value::Null | Value::Bool(false) => Ok(Vec::new()),
        Value::Bool(true) => Ok(vec![format!("--{key}")]),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                out.push(format!("--{key}"));
                out.push(word(key, item)?);
            }
            Ok(out)
        }
        other => Ok(vec![format!("--{key}"), word(key, other)?]),
    }
}

/// One value as the word it is spelled as, or the reason a command line has
/// no spelling for it.
fn word(key: &str, value: &Value) -> Result<String, ErrorData> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(_) => Err(unspellable(key, "a boolean where a word was wanted")),
        Value::Null => Err(unspellable(key, "null")),
        Value::Array(_) => Err(unspellable(key, "an array inside an array")),
        Value::Object(_) => Err(unspellable(key, "an object")),
    }
}

fn unspellable(key: &str, what: &str) -> ErrorData {
    ErrorData::invalid_params(
        format!("`{key}` is {what}, and a command line has no spelling for it"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(name: &str, schema: Value) -> ToolDescriptor {
        let Value::Object(input_schema) = schema else {
            unreachable!("a schema is an object")
        };
        serde_json::from_value(serde_json::json!({
            "name": name,
            "description": "what it does",
            "inputSchema": input_schema,
            "annotations": {"readOnlyHint": true, "destructiveHint": false},
        }))
        .expect("the descriptor parses")
    }

    fn object() -> Value {
        serde_json::json!({"type": "object"})
    }

    fn arguments(value: Value) -> Option<JsonObject> {
        match value {
            Value::Object(map) => Some(map),
            _ => unreachable!("arguments are an object"),
        }
    }

    fn one(extension: &str, name: &str, schema: Value) -> Produced {
        let mut out = Vec::new();
        offer(&mut out, extension, vec![descriptor(name, schema)]);
        out.pop().expect("one tool")
    }

    #[test]
    fn a_tool_is_named_for_its_extension_and_routes_back_to_a_verb() {
        let produced = one("tower", "brief", object());
        assert_eq!(produced.name(), "tower__brief");
        let call = produced.call(None).expect("no arguments is a bare line");
        assert_eq!(call.args, vec!["tower", "brief"]);
        assert!(call.cwd.is_none(), "the server's own directory");
        assert_eq!(
            call.route,
            Route::Produced,
            "the undoable gate is the args array's"
        );
        // And what the client is offered says what the descriptor said.
        let tool = produced.tool();
        assert_eq!(tool.name, "tower__brief");
        let annotations = tool.annotations.expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, None, "unsaid stays unsaid");
    }

    /// Two extensions cannot take one name, and neither can take `ff`. The
    /// first declared keeps it and the rest of a list stands.
    #[test]
    fn a_name_already_taken_is_dropped_and_the_rest_of_the_list_stands() {
        let mut out = Vec::new();
        offer(&mut out, "a__b", vec![descriptor("c", object())]);
        offer(
            &mut out,
            "a",
            vec![descriptor("b__c", object()), descriptor("d", object())],
        );
        let names: Vec<&str> = out.iter().map(Produced::name).collect();
        assert_eq!(names, vec!["a__b__c", "a__d"]);
        assert_eq!(out[0].extension, "a__b", "the first declared kept it");
    }

    #[test]
    fn the_arguments_object_becomes_a_command_line() {
        let produced = one(
            "tower",
            "file",
            serde_json::json!({"type": "object", "positional": ["title"]}),
        );
        let call = produced
            .call(arguments(serde_json::json!({
                "title": "parser: skeleton",
                "board": "ff tower",
                "depth": 3,
                "urgent": true,
                "draft": false,
                "quiet": null,
                "label": ["a", "b"],
            })))
            .expect("a line");
        assert_eq!(
            call.args,
            vec![
                "tower",
                "file",
                "parser: skeleton",
                "--board",
                "ff tower",
                "--depth",
                "3",
                "--urgent",
                "--label",
                "a",
                "--label",
                "b",
            ],
            "positionals first, then an option per property in the order they arrived"
        );
    }

    /// A positional left out shifts every word after it, so the first gap
    /// ends the positionals.
    #[test]
    fn a_missing_positional_ends_the_line_rather_than_shifting_it() {
        let produced = one(
            "tower",
            "brief",
            serde_json::json!({"type": "object", "positional": ["flight", "field"]}),
        );
        let call = produced
            .call(arguments(serde_json::json!({"field": "title"})))
            .expect("a line");
        assert_eq!(call.args, vec!["tower", "brief"]);
        let call = produced
            .call(arguments(
                serde_json::json!({"flight": 98, "field": "title"}),
            ))
            .expect("a line");
        assert_eq!(call.args, vec!["tower", "brief", "98", "title"]);
    }

    #[test]
    fn an_argument_a_command_line_cannot_spell_is_a_protocol_error() {
        let produced = one("tower", "file", object());
        assert!(
            produced
                .call(arguments(serde_json::json!({"where": {"board": "x"}})))
                .is_err(),
            "an object has no spelling"
        );
        assert!(
            produced
                .call(arguments(serde_json::json!({"label": [["a"]]})))
                .is_err(),
            "nor an array inside one"
        );
        assert!(
            produced
                .call(arguments(serde_json::json!({"--force": true})))
                .is_err(),
            "nor a property name that is not an option's"
        );
    }
}
