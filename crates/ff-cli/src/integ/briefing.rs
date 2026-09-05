//! The once-per-session briefing, and the guards that keep it true.
//!
//! One text feeds every client. What differs per vendor is only how it is
//! delivered — plain stdout for Claude and Codex, a JSON field for Gemini
//! and Cursor — which is why the envelope is the adapter's job and this is
//! not.
//!
//! A declared extension may add one line of its own, which is the other
//! half of this file. It rides the same boundaries and the same marker the
//! notice does, because it is briefing: an agent pays for it once per
//! audience, so it is budgeted the way the notice is.
//!
//! The guards at the bottom cover the shipped skill too. Both texts are
//! prose an agent reads as instructions, both rot the same silent way, and
//! the skill is the larger surface by an order of magnitude — so the check
//! that every command in them is one the CLI still takes belongs to both.

use std::path::Path;

use crate::manifest::Briefing;

/// What the agent is told, once per session, when a client's context-start
/// event gives fufu somewhere to put it.
///
/// This is the always-on contract and nothing more: the four verbs that
/// write, the git rule, and where the authority is. Everything past it —
/// recovery, rewriting, conflicts, the machine surface — lives in the
/// shipped skill (`integ/skill.md`), which costs nothing until a client
/// decides it is wanted. The two are budgeted differently on purpose, and
/// that is the whole reason the split exists.
///
/// Every command here is real and spelled the way the CLI takes it — a
/// retired or mistyped form teaches the agent to fail. Keep it short: this
/// is context the agent pays for on every session.
///
/// Every line is quoted from the CLI, and each source carries a matching
/// `// agent notice quotes this` marker in cli.rs — retiring a verb or
/// renaming a flag there is an edit here too. Adding a command to this text
/// means adding that marker at its definition, so the trail stays two-way:
/// `grep -rn "agent notice" crates/ff-cli/src`.
pub const NOTICE: &str = "\
fufu (`ff`) is capturing this repository: the working copy is snapshotted before every \
tool action, so no edit can lose file state. Work directly — no backup copies, no \
hedging.

Use `ff`, not `git`, for anything that writes. `ff commit -m \"…\"` closes the open \
change — no add, no staging, the working copy is the change. `ff switch <branch>` moves. \
`ff undo` takes back the last operation. `ff restore <path>` discards a file's edits. \
Anything else git does: `ff git <args…>`, which snapshots and then runs git verbatim.

Reading with git is fine. `ff status`, `ff log`, and `ff diff` say more than their git \
counterparts.

When an `ff` tool is offered, call it with the same words; while it is up, `ff` in the \
shell is refused.

Every verb's own `--help` is the authority on it.
";

// ---- what a declared extension adds ----------------------------------------

/// The most a declared extension's briefing line may run to, in characters.
///
/// The notice is budgeted because it is context every session pays for, and
/// an extension's line is spending the same budget on the same terms. Two
/// hundred and forty characters is a sentence saying what the extension is
/// for and how it is spelled, which is the whole job of a line here. An
/// extension with more to teach ships a skill, which costs the agent
/// nothing until it is read.
pub const LINE_CAP: usize = 240;

/// The line every declared extension contributes, in the order the registry
/// holds them — which is the order they were declared, and load-bearing for
/// exactly this reason.
///
/// The manifest picks the arm. A string in `briefing` is the line itself; a
/// `true` there means run `ff-<name> briefing` in `cwd`, with the `FF_*`
/// variables set, and take its stdout. Absent means the extension has
/// nothing to say, which is the common answer.
///
/// Everything that can go wrong produces no line and costs the caller
/// nothing: a binary that has left PATH, one that will not start, one that
/// fails or hangs or prints something that is not one line, and a line past
/// [`LINE_CAP`]. That is `ff trigger`'s doctrine applied to the one place
/// fufu invites an extension to speak into an agent's context, and it is
/// why nothing here returns an error.
pub fn extension_lines(cwd: &Path, repo: Option<&Path>, session: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    for declared in crate::registry::read().declared() {
        let said = match &declared.manifest.briefing {
            Some(Briefing::Line(line)) => Some(line.clone()),
            Some(Briefing::Ask(true)) => asked(declared.name(), cwd, repo, session),
            Some(Briefing::Ask(false)) | None => None,
        };
        if let Some(line) = said.as_deref().and_then(usable) {
            lines.push(line);
        }
    }
    lines
}

/// `ff-<name> briefing`, under fufu's time box, with its stdout as text.
fn asked(name: &str, cwd: &Path, repo: Option<&Path>, session: Option<&str>) -> Option<String> {
    let said = crate::ext::ask(&crate::ext::Ask {
        name,
        verb: "briefing",
        rest: &[],
        cwd,
        repo,
        session,
        stdin: &[],
        budget: crate::ext::BUDGET,
    })?;
    String::from_utf8(said).ok()
}

/// One line, or nothing.
///
/// Surrounding whitespace is fufu's to trim, since a binary that ends its
/// line with a newline is the normal case. What is left has to be a single
/// line inside the cap. A text carrying its own newlines is refused rather
/// than folded, because it would let one extension shape the briefing
/// instead of contributing a line to it; a line past the cap is dropped
/// whole rather than cut, because half a sentence is still prose the agent
/// reads as instructions.
fn usable(said: &str) -> Option<String> {
    let line = said.trim();
    if line.is_empty() || line.contains('\n') || line.chars().count() > LINE_CAP {
        return None;
    }
    Some(line.to_string())
}

// ---- the notice is a contract with the CLI ---------------------------------

/// `NOTICE` and the shipped skill are prose an agent reads as instructions,
/// so they rot in a way the compiler cannot see: a retired verb or a renamed
/// flag still reads fine and simply teaches the agent to fail. These guards
/// make it fail here instead — clap is the authority on what either text is
/// allowed to say.
#[cfg(test)]
mod notice {
    use clap::{CommandFactory, Parser};

    use super::{LINE_CAP, NOTICE, usable};
    use crate::cli::Cli;
    use crate::integ::skill::SKILL;

    /// cli.rs as text, for the marker trail. Read as source rather than
    /// through clap because a comment is exactly what clap discards.
    const CLI_SRC: &str = include_str!("../cli.rs");
    const MARKER: &str = "// agent notice quotes this";

    /// Every `ff …` a text spells, as argv. Placeholders (`<path>`,
    /// `"…"`) become a value, since the point is the grammar around them.
    fn quoted(text: &str) -> Vec<Vec<String>> {
        text.split('`')
            // Odd fields are the ones between a pair of backticks.
            .skip(1)
            .step_by(2)
            .filter(|span| *span == "ff" || span.starts_with("ff "))
            .map(|span| {
                span.split_whitespace()
                    .map(|tok| {
                        if tok.starts_with('<') || tok.starts_with('"') {
                            "x".to_string()
                        } else {
                            tok.to_string()
                        }
                    })
                    .collect()
            })
            .collect()
    }

    fn find_arg<'a>(cmd: &'a clap::Command, flag: &str) -> Option<&'a clap::Arg> {
        cmd.get_arguments().find(|arg| {
            if let Some(long) = flag.strip_prefix("--") {
                arg.get_long() == Some(long)
            } else {
                flag.strip_prefix('-')
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| arg.get_short() == Some(c))
            }
        })
    }

    /// The guard that the two rotted spellings needed. Parsing alone is not
    /// enough: retired surface (`-m`, `--ops`, `ff checkout`) is *declared*,
    /// hidden, so that typing it reaches an answer — it parses and then
    /// refuses. So hidden is disqualifying here, not just unknown.
    #[test]
    fn only_live_documented_surface() {
        let root = Cli::command();
        // The MCP tool's card is prose an agent reads the same way, and
        // it rots the same way, so it is held to the same rule.
        let card = {
            use crate::cmd::mcp::describe::{CONTRACT, DOCTRINE, LANDMINES, RECOVERY};
            format!("{CONTRACT}\n{DOCTRINE}\n{RECOVERY}\n{LANDMINES}")
        };
        let mut commands = quoted(NOTICE);
        assert!(
            commands.len() >= 8,
            "the notice stopped teaching verbs: {commands:?}"
        );
        let from_card = quoted(&card);
        assert!(
            from_card.len() >= 20,
            "the card stopped teaching verbs: {from_card:?}"
        );
        commands.extend(from_card);
        for tokens in &commands {
            let line = tokens.join(" ");
            let mut cmd = &root;
            let mut rest = &tokens[1..];
            while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
                assert!(
                    !sub.is_hide_set(),
                    "`{line}` names {:?}, which is hidden — retired or undocumented \
                     surface must not be taught to an agent",
                    sub.get_name()
                );
                cmd = sub;
                rest = &rest[1..];
            }
            for flag in rest.iter().filter(|tok| tok.starts_with('-')) {
                let arg = find_arg(cmd, flag)
                    .or_else(|| find_arg(&root, flag))
                    .unwrap_or_else(|| panic!("`{line}` passes {flag}, which does not exist"));
                assert!(
                    !arg.is_hide_set(),
                    "`{line}` passes {flag}, which is hidden — retired or undocumented \
                     surface must not be taught to an agent"
                );
            }
            Cli::try_parse_from(tokens)
                .unwrap_or_else(|err| panic!("`{line}` does not parse:\n{err}"));
        }
    }

    /// The same guard over the skill. It teaches most of the command
    /// surface rather than nine verbs of it, so this is where a retired
    /// verb is overwhelmingly likely to be found still being taught.
    #[test]
    fn the_skill_teaches_only_live_surface() {
        let root = Cli::command();
        let commands = quoted(SKILL);
        assert!(
            commands.len() >= 40,
            "the skill stopped teaching the surface the notice cannot afford: {}",
            commands.len()
        );
        for tokens in &commands {
            let line = tokens.join(" ");
            let mut cmd = &root;
            let mut rest = &tokens[1..];
            while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
                assert!(
                    !sub.is_hide_set(),
                    "the skill names {:?} in `{line}`, which is hidden — retired or \
                     undocumented surface must not be taught to an agent",
                    sub.get_name()
                );
                cmd = sub;
                rest = &rest[1..];
            }
            for flag in rest.iter().filter(|tok| tok.starts_with('-')) {
                let arg = find_arg(cmd, flag)
                    .or_else(|| find_arg(&root, flag))
                    .unwrap_or_else(|| {
                        panic!("the skill passes {flag} in `{line}`, which does not exist")
                    });
                assert!(
                    !arg.is_hide_set(),
                    "the skill passes {flag} in `{line}`, which is hidden — retired or \
                     undocumented surface must not be taught to an agent"
                );
            }
            Cli::try_parse_from(tokens)
                .unwrap_or_else(|err| panic!("the skill's `{line}` does not parse:\n{err}"));
        }
    }

    /// The other half of the trail: a command in the notice has a marker at
    /// its definition, so whoever retires it there sees this text named.
    #[test]
    fn every_quoted_command_is_marked_in_cli_rs() {
        let markers: Vec<&str> = CLI_SRC
            .lines()
            .map(str::trim_start)
            .filter(|line| line.starts_with(MARKER))
            .collect();
        assert!(
            markers.len() >= 8,
            "the marker trail is gone from cli.rs; the notice has nothing pointing at it"
        );
        for tokens in quoted(NOTICE) {
            // Bare `ff` — the notice names the tool before it names a verb.
            let Some(verb) = tokens.get(1) else { continue };
            assert!(
                markers.iter().any(|m| m.contains(&format!("`ff {verb}"))),
                "the notice teaches `ff {verb}` but no `{MARKER}` in cli.rs claims it: \
                 add one at its definition"
            );
        }
    }

    /// The notice is context the agent pays for on every session. There is no
    /// exact token count to assert, so the budget is bytes: roughly 160
    /// tokens, and a rewrite that doubles it has to say so here. The number
    /// came down when the skill took the advanced surface off it; growing
    /// it back is choosing to charge every session for something one
    /// session in twenty needs.
    #[test]
    fn stays_within_its_budget() {
        assert!(
            NOTICE.len() <= 800,
            "the notice is {} bytes; trim it or raise the budget deliberately",
            NOTICE.len()
        );
    }

    /// A declared extension spends the same budget the notice does, so its
    /// line is capped where the notice is budgeted. The number is stated
    /// against the notice rather than on its own: a line a third of the
    /// whole always-on text is already a lot for one extension to ask of
    /// every session, and a machine with several of them declared would be
    /// paying it several times over.
    #[test]
    fn the_extension_line_cap_sits_under_the_notices_budget() {
        assert!(
            LINE_CAP * 3 <= NOTICE.len(),
            "an extension's {LINE_CAP} characters are no longer small against the notice's {} \
             bytes; move one or the other deliberately",
            NOTICE.len()
        );
    }

    /// The cap is a cap and not a truncation: a line past it is dropped
    /// whole, because half a sentence is still prose the agent reads as
    /// instructions.
    #[test]
    fn a_line_past_the_cap_is_dropped_rather_than_cut() {
        let at = "x".repeat(LINE_CAP);
        assert_eq!(usable(&at).as_deref(), Some(at.as_str()));
        assert_eq!(usable(&"x".repeat(LINE_CAP + 1)), None);
        // Characters, not bytes: a line in another script is not charged
        // for the width of its encoding.
        assert!(usable(&"é".repeat(LINE_CAP)).is_some());
    }

    /// One line means one line. Whitespace around it is fufu's to trim, and
    /// a text carrying its own newlines is refused rather than folded.
    #[test]
    fn an_extension_contributes_one_line_or_nothing() {
        assert_eq!(
            usable("  Work is filed as flights.\n").as_deref(),
            Some("Work is filed as flights.")
        );
        assert_eq!(usable(""), None);
        assert_eq!(usable("   \n\n"), None);
        assert_eq!(usable("one\ntwo"), None);
        assert_eq!(usable("one\r\ntwo"), None);
    }

    /// The half of the `--at` lesson that moved out of the notice with the
    /// verb. An id goes to `--at-op` and `--at` takes a time; the two were
    /// confused once, in a text an agent reads as instructions.
    #[test]
    fn the_skill_keeps_the_at_lesson_straight() {
        assert!(
            SKILL.contains("--at-op <id>"),
            "the skill teaches where an operation id goes"
        );
        assert!(
            !SKILL.contains("--at <id>"),
            "an id never goes to --at; --at takes a time"
        );
    }

    /// The root `--help` ends with a block routing an agent that has no
    /// skill in its context to go and print one. That is the same class of
    /// text as the notice — prose an agent reads as instructions — so it
    /// gets the same guard: the command it names is one clap still takes.
    /// Without this, retiring the flag teaches an agent to fail instead of
    /// failing here.
    #[test]
    fn the_help_block_routes_an_agent_to_a_command_that_runs() {
        let block = crate::help::ROOT_EXAMPLES
            .split_once("Are you an agent")
            .expect("the root help still carries the agent block")
            .1;
        // Two columns: the command, then whitespace, then what it is for.
        let commands: Vec<Vec<String>> = block
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("ff "))
            .map(|line| {
                line.split("  ")
                    .next()
                    .unwrap_or(line)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        assert!(
            !commands.is_empty(),
            "the agent block stopped naming a command to run"
        );
        for tokens in &commands {
            let line = tokens.join(" ");
            Cli::try_parse_from(tokens)
                .unwrap_or_else(|err| panic!("the agent block's `{line}` does not parse:\n{err}"));
        }
    }

    /// The skill is paid for only when it is read, so its budget is loose —
    /// but it is a budget, because an unwatched manual grows until it is
    /// one nobody finishes.
    #[test]
    fn the_skill_stays_within_its_budget() {
        assert!(
            SKILL.len() <= 16_000,
            "the skill is {} bytes; trim it or raise the budget deliberately",
            SKILL.len()
        );
    }
}
