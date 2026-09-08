//! fufu's own tools: seven, typed, generated from the command tree.
//!
//! `status`, `pull`, `push`, `undo`, `redo`, `explain`, and `help` are
//! the verbs where the shell adds nothing — fixed and short inputs, no
//! output an agent would pipe, and a result whose structure matters more
//! than its text. Every other verb is the shell. Each descriptor here is
//! read off the same clap tree `ff --help` reads, so a flag added to a verb
//! is a field added to its tool, and a description is the page's first
//! paragraph; the test at the bottom pins that every property is an
//! argument the live verb accepts and that a call spells into a line clap
//! parses.
//!
//! The tools go out through the same [`Typed`] an extension's produced
//! tools go out through, so there is one route and one set of promises for
//! fufu's and theirs: the object becomes a command line in one place, `cwd`
//! is lifted in one place, and the child is the same ordinary invocation.
//!
//! `help` has no clap variant — it is clap's auto subcommand, present only
//! on a built tree, and `main` routes `ff help <extension>` by hand — so it
//! is shaped here: one positional `verb`, an array of words, so `["op",
//! "log"]` spells `ff help op log` and nothing spells bare `ff help`, the
//! root map. Now the card is gone, that is the one tool that lists verbs.

use clap::{ArgAction, CommandFactory};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{Map, Value};

use super::tools::{CWD, POSITIONAL, Typed, cwd_property};

/// The four hints, all stated: MCP defaults `destructive` and `open_world`
/// to true when unstated, and a tool that says nothing is a tool that says
/// the worst.
#[derive(Clone, Copy)]
struct Hints {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

/// The six clap-backed verbs, in the order they are listed.
///
/// `push` is destructive because its page opens by calling it the one
/// act undo cannot take back. `pull` is one undoable operation, so not; both
/// reach a remote, so open-world. `undo` and `redo` repeat, so neither is
/// idempotent.
const VERBS: [(&str, Hints); 6] = [
    (
        "status",
        Hints {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        },
    ),
    (
        "pull",
        Hints {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
        },
    ),
    (
        "push",
        Hints {
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
        },
    ),
    (
        "undo",
        Hints {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        },
    ),
    (
        "redo",
        Hints {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        },
    ),
    (
        "explain",
        Hints {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        },
    ),
];

/// The hand-shaped seventh.
const HELP: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: true,
    open_world: false,
};

const HELP_DESCRIPTION: &str = "A verb's help page as text, the authority on it: what it does, \
    every flag, and examples. `verb` is the words after `ff help`: `[\"commit\"]`, or `[\"op\", \
    \"log\"]`. With none, the map of every verb.";

/// The seven, in order: `status, pull, push, undo, redo, explain, help`.
pub fn own() -> Vec<Typed> {
    // Unbuilt, the way every other walk of the tree is: `build` is what
    // grows the frame, and nothing here needs the globals or the auto help
    // arg it would add.
    let root = crate::cli::Cli::command();
    let mut out: Vec<Typed> = VERBS
        .iter()
        .map(|(name, hints)| {
            let cmd = root
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("{name} is served but not live"));
            let (properties, positional, required) = schema(cmd);
            let tool = Tool::new(
                name.to_string(),
                description(cmd),
                input_schema(properties, &positional, required),
            )
            .with_annotations(annotations(*hints));
            Typed::own(name, positional, tool)
        })
        .collect();

    let mut properties = Map::new();
    properties.insert(
        "verb".into(),
        serde_json::json!({
            "type": "array",
            "items": {"type": "string"},
            "description": "The words after `ff help`, one per item",
        }),
    );
    let positional = vec!["verb".to_string()];
    let tool = Tool::new(
        "help",
        HELP_DESCRIPTION,
        input_schema(properties, &positional, Vec::new()),
    )
    .with_annotations(annotations(HELP));
    out.push(Typed::own("help", positional, tool));
    out
}

/// One property per argument the verb takes: the long name of an option,
/// the id of a positional. The positionals come back separately, in
/// declaration order, for the schema's [`POSITIONAL`] keyword, and so do
/// the names clap requires, for `required`.
///
/// Skipped: a global (`-C`, `--json`, `--session` are the server's, not the
/// tool's), a hidden one (retired surface), the auto help, a short-only
/// option (nothing spells it), and any action but the three a JSON value
/// has a spelling for. On an unbuilt subcommand none of the globals or the
/// help arg are present anyway; the guards make this right on a built tree
/// too.
fn schema(cmd: &clap::Command) -> (Map<String, Value>, Vec<String>, Vec<String>) {
    let mut properties = Map::new();
    let mut positional = Vec::new();
    let mut required = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_global_set() || arg.is_hide_set() || arg.get_id() == "help" {
            continue;
        }
        let kind = match arg.get_action() {
            ArgAction::SetTrue => "boolean",
            ArgAction::Set => "string",
            ArgAction::Append => "array",
            _ => continue,
        };
        let name = if arg.is_positional() {
            positional.push(arg.get_id().to_string());
            arg.get_id().to_string()
        } else {
            match arg.get_long() {
                Some(long) => long.to_string(),
                None => continue,
            }
        };
        let mut property = Map::new();
        property.insert("type".into(), Value::from(kind));
        if kind == "array" {
            property.insert("items".into(), serde_json::json!({"type": "string"}));
        }
        let mut text = arg.get_help().map(ToString::to_string).unwrap_or_default();
        if kind != "boolean"
            && !arg.is_positional()
            && let Some(value_name) = arg.get_value_names().and_then(|names| names.first())
        {
            text.push_str(&format!(" <{value_name}>"));
        }
        if !text.is_empty() {
            property.insert("description".into(), Value::from(text));
        }
        if arg.is_required_set() {
            required.push(name.clone());
        }
        properties.insert(name, Value::Object(property));
    }
    (properties, positional, required)
}

/// The properties as the tool's `inputSchema`: `cwd` last, the positionals
/// named, nothing else allowed.
fn input_schema(
    mut properties: Map<String, Value>,
    positional: &[String],
    required: Vec<String>,
) -> JsonObject {
    properties.insert(CWD.into(), cwd_property());
    let mut schema = Map::new();
    schema.insert("type".into(), Value::from("object"));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::from(required));
    }
    if !positional.is_empty() {
        schema.insert(POSITIONAL.into(), Value::from(positional.to_vec()));
    }
    schema.insert("additionalProperties".into(), Value::from(false));
    schema
}

/// The page's first paragraph on one line, or the one-line `about` for a
/// verb with no page.
fn description(cmd: &clap::Command) -> String {
    let text = cmd
        .get_long_about()
        .or_else(|| cmd.get_about())
        .map(ToString::to_string)
        .unwrap_or_default();
    let first = text.split("\n\n").next().unwrap_or_default();
    first
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn annotations(hints: Hints) -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(hints.read_only)
        .destructive(hints.destructive)
        .idempotent(hints.idempotent)
        .open_world(hints.open_world)
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    fn properties(tool: &Tool) -> &Map<String, Value> {
        tool.input_schema["properties"]
            .as_object()
            .expect("properties")
    }

    /// One sample value per property, of the type the schema says.
    fn sample(property: &Value) -> Value {
        match property["type"].as_str() {
            Some("boolean") => Value::Bool(true),
            Some("array") => serde_json::json!(["x"]),
            _ => Value::from("x"),
        }
    }

    #[test]
    fn the_seven_are_named_bare_in_order_and_help_spells_its_words() {
        let tools = own();
        let names: Vec<&str> = tools.iter().map(Typed::name).collect();
        assert_eq!(
            names,
            vec!["status", "pull", "push", "undo", "redo", "explain", "help"]
        );
        let help = own().pop().expect("help");
        let bare = help.call(None).expect("no arguments is bare help");
        assert_eq!(bare.args, vec!["help"]);
        let Value::Object(arguments) = serde_json::json!({"verb": ["op", "log"]}) else {
            unreachable!()
        };
        let page = help.call(Some(arguments)).expect("a line");
        assert_eq!(page.args, vec!["help", "op", "log"]);
    }

    /// The description is the page's first paragraph on one line.
    #[test]
    fn a_description_is_the_pages_first_paragraph_on_one_line() {
        let tools = own();
        let status = tools[0].tool();
        let description = status.description.as_deref().expect("a description");
        assert!(
            description.starts_with("Where you are and what is uncommitted"),
            "{description}"
        );
        assert!(!description.contains('\n'), "{description}");
        assert!(
            description.ends_with("`ff st` is the short spelling."),
            "one paragraph and no more: {description}"
        );
        // A verb with no page yields its one-line about.
        let explain = tools[5].tool();
        assert_eq!(
            explain.description.as_deref(),
            Some("Look up an error id and see what it means")
        );
        // And every one states all four hints.
        for typed in &tools {
            let tool = typed.tool();
            let annotations = tool.annotations.expect("annotations");
            assert!(annotations.read_only_hint.is_some(), "{}", tool.name);
            assert!(annotations.destructive_hint.is_some(), "{}", tool.name);
            assert!(annotations.idempotent_hint.is_some(), "{}", tool.name);
            assert!(annotations.open_world_hint.is_some(), "{}", tool.name);
        }
        assert_eq!(
            tools[2].tool().annotations.unwrap().destructive_hint,
            Some(true),
            "push is the one act undo cannot take back"
        );
    }

    /// Every property is an argument the live verb accepts, and an object
    /// with one value per property spells into a line clap parses.
    #[test]
    fn every_property_is_an_argument_the_live_verb_accepts_and_a_call_parses() {
        let root = crate::cli::Cli::command();
        for typed in own() {
            let tool = typed.tool();
            let name = tool.name.to_string();
            assert_eq!(
                tool.input_schema["additionalProperties"],
                Value::Bool(false),
                "{name}"
            );
            let props = properties(&tool);
            assert!(props.contains_key(CWD), "{name} takes cwd");
            let positional: Vec<String> = tool
                .input_schema
                .get(POSITIONAL)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().unwrap().to_string())
                        .collect()
                })
                .unwrap_or_default();
            if name != "help" {
                let cmd = root.find_subcommand(&name).expect("live");
                for key in props.keys().filter(|key| *key != CWD) {
                    let found = cmd.get_arguments().any(|arg| {
                        if positional.contains(key) {
                            arg.is_positional() && arg.get_id() == key.as_str()
                        } else {
                            arg.get_long() == Some(key.as_str())
                        }
                    });
                    assert!(found, "{name}.{key} is not an argument ff {name} accepts");
                }
            }

            let mut arguments = JsonObject::new();
            for (key, property) in props {
                // `--at` and `--at-op` conflict, which the schema does not
                // say and the child answers with an envelope.
                if name == "status" && key == "at" {
                    continue;
                }
                let value = if key == CWD {
                    Value::from("/tmp")
                } else if name == "help" {
                    // A page that exists, since clap answers a word it does
                    // not know with the same error a misspelled verb gets.
                    serde_json::json!(["op", "log"])
                } else {
                    sample(property)
                };
                arguments.insert(key.clone(), value);
            }
            let call = typed.call(Some(arguments)).expect("a line");
            assert_eq!(call.cwd.as_deref(), Some("/tmp"), "{name}");
            assert!(
                !call.args.iter().any(|word| word == "--cwd"),
                "{name}: cwd is the server's option, never the tool's: {:?}",
                call.args
            );
            assert_eq!(call.args[0], name);
            let mut argv = vec!["ff".to_string()];
            argv.extend(call.args.iter().cloned());
            let parsed = crate::cli::Cli::try_parse_from(&argv);
            if name == "help" {
                let err = parsed.err().expect("help displays");
                assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp, "{argv:?}");
            } else {
                parsed.unwrap_or_else(|err| panic!("{argv:?} does not parse: {err}"));
            }
        }
    }

    #[test]
    fn the_properties_are_the_verbs_own_flags() {
        let tools = own();
        let keys =
            |i: usize| -> Vec<String> { properties(&tools[i].tool()).keys().cloned().collect() };
        assert_eq!(keys(0), vec!["at-op", "at", "cwd"]);
        assert_eq!(keys(1), vec!["no-fetch", "cwd"]);
        assert_eq!(keys(2), vec!["dry-run", "to", "cwd"]);
        assert_eq!(keys(3), vec!["cwd"]);
        assert_eq!(keys(4), vec!["cwd"]);
        assert_eq!(keys(5), vec!["id", "list", "cwd"]);
        assert_eq!(
            tools[5].tool().input_schema[POSITIONAL],
            serde_json::json!(["id"])
        );
        assert_eq!(keys(6), vec!["verb", "cwd"]);
        let push = tools[2].tool();
        assert_eq!(
            properties(&push)["to"]["description"],
            "Send to this remote, and record that the branch answers to it <remote>"
        );
        assert!(push.input_schema.get("required").is_none(), "none required");
    }
}
