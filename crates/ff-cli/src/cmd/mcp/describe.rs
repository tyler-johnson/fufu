//! The one tool, and the description it carries.
//!
//! A client shows the model about the first two thousand characters of a
//! tool's description — Claude Code says so in its `/mcp` panel, and the
//! docs say nothing — so the description is a card that fits under that,
//! not the manual. Everything past the card is one call away: `ff help
//! <verb>` through the tool itself, and the shipped skill for recovery and
//! rewriting.
//!
//! The card is assembled from single sources rather than written a second
//! time where it can be: the verb list is `help::GROUPS` walked against
//! the live command tree, exactly as `ff --help` and the CLI reference
//! are, and the extensions line is the manifests the registry holds. The
//! contract, the doctrine line, and the recovery and landmine digests are
//! hand-written here and guarded the way the briefing is — every `ff …`
//! they spell has to parse against the live CLI. A test pins the whole
//! under the client's cut, so it cannot quietly grow past it, and a second
//! one pins it there against a registry built to be hostile: the card is
//! the only part of the tool a person's file can lengthen.

use std::fmt::Write as _;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};

use crate::help;

/// The tool's name, which a client prefixes with the server's:
/// [`CLAUDE_TOOL`] in Claude Code.
pub const NAME: &str = "ff";

/// The tool as Claude Code names it once the plugin's server is up: the
/// plugin, the server, and [`NAME`]. The `fufu.toolPolicy` refusal spells
/// this so the agent can call it without a lookup.
pub const CLAUDE_TOOL: &str = "mcp__plugin_fufu_fufu__ff";

/// The contract paragraph: how to call the tool and how to read what
/// comes back. Nothing here can be learned from `ff help <verb>`.
pub(crate) const CONTRACT: &str = "\
fufu (`ff`), a friendlier interface to plain git. `args` is one word per item: \
`ff commit -m \"…\"` is `[\"commit\", \"-m\", \"…\"]`. The \
result is fufu's envelope, `{\"ff\":1,\"cmd\":…,\"data\":…}` or \
`{…,\"error\":{\"id\",\"message\",\"exits\"}}`; `isError` follows the exit \
code. `ff explain <id>` expands `error.id`. `held/*`: nothing moved, a person is \
needed; stop and say so. `ref/contended`: run the same call once more. A verb's `--help` returns its \
page as text. Shell only: git, update, watch, hook, unhook, mcp, extension.";

/// The doctrine, in one breath: the briefing's four verbs and the git
/// rule, for a client that never shows the server's instructions.
pub(crate) const DOCTRINE: &str = "\
Use this tool, not git, for anything that writes. The working copy is the change: `ff commit` closes it, no staging; `ff switch <branch>` moves; `ff undo` takes back the last \
operation; `ff restore <path>` discards a file's edits. Reading with git is fine.";

/// The skill's recovery table in one line, and where the rest is. The
/// skill is named conditionally: Cursor and Gemini read none, and a
/// `--settings` Claude install has none, so `ff help` is the path that is
/// always there.
pub(crate) const RECOVERY: &str = "\
Recovery: `ff undo` repeats and `ff redo` goes forward; \
`ff op restore <id>` lands the repo on one operation; `ff restore <path> --at-op <id>` brings \
a file back. Ids: `ff history`, `ff evolog`, `ff op log`. The full table, rewriting closed \
commits, and held rewrites: the `fufu` skill when the client has one.";

/// The skill's landmines, in one line.
pub(crate) const LANDMINES: &str = "\
Landmines: never stage or stash — path-limited `ff commit` and `ff switch <branch>` replace them. Never \
`rebase -i` — `ff edit <rev>`, `ff done`, `ff absorb`, `ff describe <rev>` cover it, undoably. \
`ff git` is the escape hatch.";

/// The tool as the client lists it. Annotations are hints, and honest ones:
/// most verbs write, nothing is destructive because every write is undoable
/// and captured first, a verb is not idempotent (`commit` twice is two
/// commits), and `sync` and `publish` reach a remote.
pub fn tool() -> Tool {
    Tool::new(NAME, description(), schema()).with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(false)
            .open_world(true),
    )
}

/// Hand-written rather than derived: two fields, and a schema generator
/// would cost a derive on a struct that exists only to be described.
fn schema() -> JsonObject {
    let value = serde_json::json!({
        "type": "object",
        "properties": {
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The command line after `ff`, one word per item; \
                                `--json` is added for you"
            },
            "cwd": {
                "type": "string",
                "description": "Directory to run in; the server's own when absent"
            }
        },
        "required": ["args"],
        "additionalProperties": false
    });
    match value {
        serde_json::Value::Object(map) => map,
        _ => unreachable!("the schema is written as an object"),
    }
}

/// The card, in the order an agent reads it: how to call, the doctrine,
/// what there is to call, and what to do when it goes wrong.
pub fn description() -> String {
    card(crate::registry::read())
}

/// [`description`] against a registry of the caller's own. The tests' door,
/// and the reason the one cached reader stays a detail of `description`.
fn card(registry: &crate::registry::Registry) -> String {
    let mut out = String::from(CONTRACT);
    out.push_str("\n\n");
    out.push_str(DOCTRINE);
    out.push_str("\n\nVerbs:\n");
    out.push_str(&verbs());
    out.push_str(&extensions(registry));
    out.push('\n');
    out.push_str(RECOVERY);
    out.push_str("\n\n");
    out.push_str(LANDMINES);
    out
}

/// How many declared extensions the card names.
///
/// The card is on a hard budget and the registry is a person's file: fufu
/// bounds neither how many extensions are on it nor how long a verb's name
/// is, so the line is capped rather than trusted. An extension past a cap
/// is served exactly as one under it — what it loses is its name here, and
/// `ff extension list` in a shell is the whole list.
const EXTENSIONS: usize = 4;

/// How many of an extension's verbs the card names.
const VERBS: usize = 6;

/// How long the line may be, whatever the two caps above let onto it.
const LINE: usize = 72;

/// The declared extensions, one line under the verb list and in the shape
/// the list gives a family: the name, then the verbs its manifest lists, in
/// the order the manifest lists them. `Extensions: tower (next, file, done,
/// …)`, where the ellipsis is verbs the cap left off.
///
/// Nothing is looked for on PATH — the registry is one cached parse — and a
/// machine that has declared nothing adds nothing to the card, which is
/// most machines. What is not on the line is what the tool refuses:
/// `child::refuse_in` reads the same registry.
///
/// The tools a declared extension produced are not named here, and the line
/// is the manifest's verbs whether an extension promised tools or not. A
/// produced tool is already in the client's own list under its own name,
/// carrying its own description and its own schema; the card exists for the
/// one tool whose args array has none of that. Naming them again would
/// spend a budget of about two thousand characters twice on one thing, and
/// would put a person's file back inside a line already capped against it.
fn extensions(registry: &crate::registry::Registry) -> String {
    let declared = registry.declared();
    if declared.is_empty() {
        return String::new();
    }
    let named: Vec<String> = declared
        .iter()
        .take(EXTENSIONS)
        .map(|entry| {
            let mut verbs: Vec<&str> = entry
                .manifest
                .verbs
                .iter()
                .take(VERBS)
                .map(|verb| verb.name.as_str())
                .collect();
            if entry.manifest.verbs.len() > VERBS {
                verbs.push("…");
            }
            format!("{} ({})", entry.name(), verbs.join(", "))
        })
        .collect();
    let mut line = format!("Extensions: {}", named.join(", "));
    if declared.len() > EXTENSIONS {
        line.push_str(", …");
    }
    format!("{}\n", cap(line))
}

/// A line held to [`LINE`] characters however long the names on it are.
/// The two caps above shape the common line; this one is what makes the
/// budget a fact rather than an expectation.
fn cap(line: String) -> String {
    if line.chars().count() <= LINE {
        return line;
    }
    let kept: String = line.chars().take(LINE - 1).collect();
    format!("{kept}…")
}

/// `help::GROUPS` against the live command tree, one line per group,
/// names only, a family's subcommands in parentheses. The same walk the
/// CLI reference makes, without the pages or the abouts: `ff help <verb>`
/// is one call away, and the names are what the card has room for.
fn verbs() -> String {
    use clap::CommandFactory;

    let root = crate::cli::Cli::command();
    let mut out = String::new();
    for group in help::GROUPS {
        let names: Vec<String> = group
            .commands
            .iter()
            .map(|row| {
                let cmd = root
                    .find_subcommand(row.name)
                    .unwrap_or_else(|| panic!("{} is grouped but not live", row.name));
                let subs: Vec<&str> = family(cmd).iter().map(|sc| sc.get_name()).collect();
                if subs.is_empty() {
                    row.name.to_string()
                } else {
                    format!("{} ({})", row.name, subs.join(", "))
                }
            })
            .collect();
        let _ = writeln!(out, "{}: {}", group.heading, names.join(", "));
    }
    out
}

/// The non-hidden subcommands of a family, in declaration order. The tree
/// is not built, so clap's own `help` is not among them.
fn family(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|sc| !sc.is_hide_set())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine that has declared nothing, which is the common one and
    /// the one the fixed card is measured against. Pinned rather than
    /// read, since the developer running this has a registry of their own.
    fn bare() -> crate::registry::Registry {
        crate::registry::load(None)
    }

    /// A registry file with `count` extensions on it, each answering to
    /// `verbs` verbs, and every name as long as `width`. The directory is
    /// returned so the caller keeps it alive.
    fn registry_of(
        count: usize,
        verbs: usize,
        width: usize,
    ) -> (tempfile::TempDir, crate::registry::Registry) {
        registry_promising(count, verbs, width, false)
    }

    /// [`registry_of`], with every extension on it promising tools or none
    /// of them doing so.
    fn registry_promising(
        count: usize,
        verbs: usize,
        width: usize,
        tools: bool,
    ) -> (tempfile::TempDir, crate::registry::Registry) {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file = dir.path().join("extensions.json");
        // A name of exactly `width` characters, distinct from every other,
        // and spelled the way `ext::valid_name` requires.
        let pad = |seed: String| {
            let mut out = seed;
            while out.chars().count() < width {
                out.push('x');
            }
            out
        };
        let extensions: Vec<serde_json::Value> = (0..count)
            .map(|i| {
                let verbs: Vec<serde_json::Value> = (0..verbs)
                    .map(|v| serde_json::json!({"name": pad(format!("e{i}v{v}")), "read_only": true}))
                    .collect();
                serde_json::json!({
                    "path": "/usr/local/bin/ff-x",
                    "declared_at": 1788462398,
                    "manifest": {
                        "name": pad(format!("e{i}")),
                        "version": "0.4.1",
                        "contract": crate::machine::CONTRACT,
                        "verbs": verbs,
                        "undoable": true,
                        "tools": tools,
                    },
                })
            })
            .collect();
        let body = serde_json::json!({
            "ff": crate::machine::CONTRACT,
            "extensions": extensions,
        });
        std::fs::write(&file, body.to_string()).expect("write registry");
        let registry = crate::registry::load(Some(&file));
        assert_eq!(registry.declared().len(), count, "the fixture is readable");
        (dir, registry)
    }

    /// Every verb `ff --help` lists is on the card, in the same grouping,
    /// so an agent and a person are taught the same surface.
    #[test]
    fn the_card_names_every_grouped_verb() {
        let text = card(&bare());
        for group in help::GROUPS {
            let heading = format!("{}: ", group.heading);
            let line = text
                .lines()
                .find(|line| line.starts_with(&heading))
                .unwrap_or_else(|| panic!("missing the group {:?}", group.heading));
            let names = &line[heading.len()..];
            for row in group.commands {
                assert!(
                    names.split(", ").any(
                        |name| name == row.name || name.starts_with(&format!("{} (", row.name))
                    ),
                    "missing the verb {:?} in {line:?}",
                    row.name
                );
            }
        }
        // Families are expanded, so `ff op log` is reachable by reading.
        assert!(
            text.contains("op (log, show, diff, restore, revert)"),
            "{text}"
        );
        // And the excluded verbs are still listed — an agent that asks for
        // one is told where to run it, which needs it to know the word.
        assert!(text.contains(", git,") || text.contains(": git,"), "{text}");
    }

    /// What a client shows the model of a description: Claude Code cuts
    /// at about 2048 characters, and the docs say nothing, so the card
    /// stays under it with a margin.
    const CUT: usize = 2_000;

    #[test]
    fn the_card_fits_under_the_clients_cut() {
        let text = card(&bare());
        let count = text.chars().count();
        assert!(
            count < CUT,
            "the card is {count} characters; the cut is {CUT}:\n{text}"
        );
        // And it is not empty by accident either.
        assert!(count > 1_500, "{count}");
        assert!(text.contains("\nRecovery: "));
        assert!(text.contains("\nLandmines: "));
        // A machine that has declared nothing says nothing about them.
        assert!(!text.contains("Extensions: "), "{text}");
    }

    /// A declared extension is named on the card with the verbs its
    /// manifest lists, so an agent knows the tool serves them before it
    /// tries one.
    #[test]
    fn the_card_names_a_declared_extension() {
        let (_dir, registry) = registry_of(1, 3, 0);
        let text = card(&registry);
        assert!(
            text.contains("\nExtensions: e0 (e0v0, e0v1, e0v2)\n"),
            "{text}"
        );
        // Under the verb list, where what there is to call is written.
        let verbs = text.find("\nVerbs:\n").expect("the verb list");
        let line = text.find("\nExtensions: ").expect("the extensions line");
        let recovery = text.find("\nRecovery: ").expect("the recovery digest");
        assert!(verbs < line && line < recovery, "{text}");
        // Past the verb cap, the rest of an extension's verbs are an
        // ellipsis rather than a longer line.
        let (_dir, registry) = registry_of(1, VERBS + 2, 0);
        assert!(
            card(&registry).contains(", e0v5, …)\n"),
            "{}",
            card(&registry)
        );
    }

    /// The budget is a fact about the card and not an expectation of the
    /// registry. Nothing in fufu bounds a person's file — not how many
    /// extensions are on it, not how long a verb's name is — so the worst
    /// case is what the cut has to hold, and this is that case: more
    /// extensions than the card names, more verbs each than it lists, and
    /// every name as long as the line it would have to fit on.
    #[test]
    fn the_card_fits_the_cut_against_a_worst_case_registry() {
        let (_dir, registry) = registry_of(64, 64, LINE);
        let text = card(&registry);
        let count = text.chars().count();
        assert!(
            count < CUT,
            "the worst case is {count} characters; the cut is {CUT}:\n{text}"
        );
        let line = text
            .lines()
            .find(|line| line.starts_with("Extensions: "))
            .expect("the extensions line");
        assert_eq!(line.chars().count(), LINE, "the line is capped: {line:?}");
        assert!(line.ends_with('…'), "and says it was cut: {line:?}");
        // The whole cost of a registry, however hostile, is that one line.
        assert_eq!(count, card(&bare()).chars().count() + LINE + 1);
    }

    /// The card names verbs, and a produced tool is not one of them: it
    /// reaches the agent as a tool of its own, in the client's own list,
    /// with a description the extension wrote. So the card is the same
    /// whether an extension promised tools or not, and the budget below is
    /// still the whole cost of a registry.
    #[test]
    fn a_promised_tool_costs_the_card_nothing() {
        let (_dir, plain) = registry_promising(3, 3, 0, false);
        let (_dir, promising) = registry_promising(3, 3, 0, true);
        assert!(
            promising
                .declared()
                .iter()
                .all(|entry| entry.manifest.tools),
            "the fixture promises tools"
        );
        assert_eq!(card(&plain), card(&promising));
    }

    #[test]
    fn the_schema_requires_args_and_nothing_else() {
        let schema = serde_json::Value::Object(schema());
        assert_eq!(schema["required"], serde_json::json!(["args"]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["args"]["items"]["type"], "string");
        assert_eq!(schema["properties"]["cwd"]["type"], "string");
    }
}
