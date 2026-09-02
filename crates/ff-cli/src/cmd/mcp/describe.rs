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
//! are. The contract, the doctrine line, and the recovery and landmine
//! digests are hand-written here and guarded the way the briefing is —
//! every `ff …` they spell has to parse against the live CLI. A test pins
//! the whole under the client's cut, so it cannot quietly grow past it.

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
fufu (`ff`), a friendlier interface to plain git. `args` is the command line after `ff`, one \
word per item: `ff commit -m \"…\"` is `[\"commit\", \"-m\", \"…\"]`. `--json` is added; the \
result is fufu's envelope, `{\"ff\":1,\"cmd\":…,\"data\":…}` or \
`{\"ff\":1,\"cmd\":…,\"error\":{\"id\",\"message\",\"exits\"}}`; `isError` follows the exit \
code. `ff explain <id>` expands `error.id`. `held/*`: nothing moved, a person is \
needed; stop and say so. `ref/contended`: run the same call once more. A verb's `--help` returns its \
page as text. Shell only: `git`, `update`, `watch`, `hook`, `unhook`, `mcp`.";

/// The doctrine, in one breath: the briefing's four verbs and the git
/// rule, for a client that never shows the server's instructions.
pub(crate) const DOCTRINE: &str = "\
Use this tool, not git, for anything that writes. The worktree is the change: `ff commit -m \"…\"` \
closes it, no staging; `ff switch <branch>` moves; `ff undo` takes back the last \
operation; `ff restore <path>` discards a file's edits. Reading with git is fine.";

/// The skill's recovery table in one line, and where the rest is. The
/// skill is named conditionally: Cursor and Gemini read none, and a
/// `--settings` Claude install has none, so `ff help` is the path that is
/// always there.
pub(crate) const RECOVERY: &str = "\
Recovery: `ff undo`, repeated, takes back work and `ff redo` goes forward; \
`ff op restore <id>` lands the repo on one operation; `ff restore <path> --at-op <id>` brings \
a file back. Ids: `ff history`, `ff evolog`, `ff op log`. The full table, rewriting closed \
commits, and held rewrites: the `fufu` skill when the client has one, every verb's `--help` always.";

/// The skill's landmines, in one line.
pub(crate) const LANDMINES: &str = "\
Landmines: never stage or stash — path-limited `ff commit` and `ff switch <branch>` replace them. Never \
`rebase -i` — `ff edit <rev>`, `ff done`, `ff absorb`, `ff describe <rev>` cover it, undoably. \
`ff git` is the escape hatch, not the habit.";

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
    let mut out = String::from(CONTRACT);
    out.push_str("\n\n");
    out.push_str(DOCTRINE);
    out.push_str("\n\nVerbs:\n");
    out.push_str(&verbs());
    out.push('\n');
    out.push_str(RECOVERY);
    out.push_str("\n\n");
    out.push_str(LANDMINES);
    out
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

    /// Every verb `ff --help` lists is on the card, in the same grouping,
    /// so an agent and a person are taught the same surface.
    #[test]
    fn the_card_names_every_grouped_verb() {
        let text = description();
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
        let text = description();
        let count = text.chars().count();
        assert!(
            count < CUT,
            "the card is {count} characters; the cut is {CUT}:\n{text}"
        );
        // And it is not empty by accident either.
        assert!(count > 1_500, "{count}");
        assert!(text.contains("\nRecovery: "));
        assert!(text.contains("\nLandmines: "));
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
