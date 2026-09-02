//! The one tool, and the description it carries.
//!
//! The description is assembled at runtime from single sources rather than
//! written a second time: the verb list is `help::GROUPS` walked against
//! the live command tree, exactly as `ff --help` and the CLI reference are;
//! the doctrine is the briefing verbatim; the recovery table and the
//! landmines are cut from the shipped skill by heading. The only new prose
//! is the contract paragraph at the top, which says how to call the tool
//! and how to read what comes back. A test pins the whole under a size
//! cap, so it cannot quietly grow past the budget that justified one tool.

use std::fmt::Write as _;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};

use crate::help;

/// The tool's name, which a client prefixes with the server's:
/// `mcp__fufu__ff` in Claude Code.
pub const NAME: &str = "ff";

/// The contract paragraph: the one piece of this text that exists nowhere
/// else. Everything an agent must know to call the tool and act on the
/// answer, and nothing it can learn from `ff help <verb>` instead.
const CONTRACT: &str = "\
fufu (`ff`), a friendlier interface to plain git, as one tool. `args` is the command line \
after `ff`, one word per item — `[\"commit\", \"-m\", \"parser: skeleton\"]` — and `--json` is \
added for you. The result is fufu's envelope, `{\"ff\":1,\"cmd\":…,\"data\":…}` on success and \
`{\"ff\":1,\"cmd\":…,\"error\":{\"id\",\"message\",\"exits\"}}` on failure, as both the text \
content and the structured content; `isError` follows the exit code. `error.id` is stable \
and `ff explain <id>` expands it. An id under `held/*` means nothing moved and a person is \
needed: stop and say so. `ref/contended` means another writer held the ref for a moment: \
run the same call once more. `ff help <verb>` returns that verb's full page as text; the \
readers that page (`log`, `evolog`, `op log`) take `-n <count>`. Six verbs are not offered \
here — `git`, `update`, `watch`, `hook`, `unhook`, `mcp` — because each owns its stream or \
wires the machine; run those in a shell. `cwd` runs the call in another directory.";

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

/// The description, in the order an agent reads it: how to call, the
/// doctrine, what there is to call, and what to do when it goes wrong.
pub fn description() -> String {
    let mut out = String::from(CONTRACT);
    out.push_str("\n\n");
    out.push_str(crate::integ::briefing::NOTICE.trim_end());
    out.push_str("\n\nVerbs:\n");
    out.push_str(&verbs());
    for heading in SKILL_SECTIONS {
        out.push('\n');
        out.push_str(
            section(crate::integ::skill::SKILL, heading)
                .expect("the skill keeps the heading; a test pins it")
                .trim_end(),
        );
        out.push('\n');
    }
    out
}

/// The two skill sections the description carries whole. Recovery is the
/// table an agent needs the moment something went wrong, and the landmines
/// are the habits git taught it that fufu refuses.
const SKILL_SECTIONS: [&str; 2] = ["## Recovery", "## Landmines"];

/// `help::GROUPS` against the live command tree, families expanded, one
/// `  name  about` row per verb under each heading. The same walk the CLI
/// reference makes, without the pages.
fn verbs() -> String {
    use clap::CommandFactory;

    let root = crate::cli::Cli::command();
    let mut out = String::new();
    for group in help::GROUPS {
        let _ = writeln!(out, "\n{}:", group.heading);
        for row in group.commands {
            let cmd = root
                .find_subcommand(row.name)
                .unwrap_or_else(|| panic!("{} is grouped but not live", row.name));
            let _ = writeln!(out, "  {}  {}", row.name, about(cmd));
            for sub in family(cmd) {
                let _ = writeln!(out, "  {} {}  {}", row.name, sub.get_name(), about(sub));
            }
        }
    }
    out
}

fn about(cmd: &clap::Command) -> String {
    cmd.get_about().map(ToString::to_string).unwrap_or_default()
}

/// The non-hidden subcommands of a family, in declaration order. The tree
/// is not built, so clap's own `help` is not among them.
fn family(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|sc| !sc.is_hide_set())
        .collect()
}

/// One `## ` section of a markdown document, heading included, up to the
/// next heading of the same level. `None` when the heading is not there.
pub(super) fn section<'a>(md: &'a str, heading: &str) -> Option<&'a str> {
    let start = md.match_indices(heading).map(|(idx, _)| idx).find(|&idx| {
        (idx == 0 || md[..idx].ends_with('\n')) && {
            let rest = &md[idx + heading.len()..];
            rest.is_empty() || rest.starts_with('\n')
        }
    })?;
    let body = &md[start + heading.len()..];
    let end = body
        .find("\n## ")
        .map_or(md.len(), |i| start + heading.len() + i + 1);
    Some(&md[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every verb `ff --help` lists is in the description, in the same
    /// grouping, so an agent and a person are taught the same surface.
    #[test]
    fn the_description_names_every_grouped_verb() {
        let text = description();
        for group in help::GROUPS {
            assert!(
                text.contains(&format!("\n{}:\n", group.heading)),
                "missing the heading {:?}",
                group.heading
            );
            for row in group.commands {
                assert!(
                    text.contains(&format!("\n  {}  ", row.name)),
                    "missing the verb {:?}",
                    row.name
                );
            }
        }
        // Families are expanded, so `ff op log` is reachable by reading.
        assert!(text.contains("\n  op log  "), "families expand: {text}");
        // And the excluded verbs are still listed — an agent that asks for
        // one is told where to run it, which needs it to know the word.
        assert!(text.contains("\n  git  "));
    }

    /// The two skill headings exist and land in the description, whole.
    #[test]
    fn both_skill_sections_are_carried() {
        let text = description();
        for heading in SKILL_SECTIONS {
            let cut = section(crate::integ::skill::SKILL, heading)
                .unwrap_or_else(|| panic!("the skill lost its {heading} section"));
            assert!(cut.starts_with(heading));
            assert!(
                !cut[heading.len()..].contains("\n## "),
                "{heading} ran into the next section: {cut}"
            );
            assert!(
                text.contains(cut.trim_end()),
                "{heading} is not in the description"
            );
        }
        assert!(
            text.contains("| `ff undo`, repeated |"),
            "the recovery table"
        );
        assert!(text.contains("**Do not stage.**"), "the landmines");
    }

    /// What no description may exceed, in characters. The measured
    /// assembly is about eight thousand, two thousand tokens and change;
    /// twelve leaves room for verbs to come and none for a second copy of
    /// the skill.
    const CAP: usize = 12_000;

    /// The budget that justified one tool over forty.
    #[test]
    fn the_description_stays_under_the_cap() {
        let text = description();
        assert!(
            text.chars().count() < CAP,
            "the description is {} characters; the cap is {CAP}",
            text.chars().count()
        );
        // And it is not empty by accident either: the contract, the
        // doctrine, and the skill cuts are each a page.
        assert!(text.chars().count() > 5_000, "{}", text.chars().count());
    }

    #[test]
    fn a_section_is_cut_at_the_next_heading() {
        let md = "# T\n\nintro\n\n## A\n\none\n\n## B\n\ntwo\n";
        assert_eq!(section(md, "## A"), Some("## A\n\none\n\n"));
        assert_eq!(section(md, "## B"), Some("## B\n\ntwo\n"));
        assert_eq!(section(md, "## C"), None);
        // A heading that is a prefix of another is not that other.
        assert_eq!(section("## Ab\n\nx\n", "## A"), None);
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
