//! The once-per-session briefing, and the guards that keep it true.
//!
//! One text feeds every client. What differs per vendor is only how it is
//! delivered — plain stdout for Claude and Codex, a JSON field for Gemini
//! and Cursor — which is why the envelope is the adapter's job and this is
//! not.

/// What the agent is told, once per session, when a client's context-start
/// event gives fufu somewhere to put it.
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
fufu (`ff`) is capturing this repository: the worktree is snapshotted before every \
tool action, so no edit can lose file state. Work directly — no backup copies, no \
hedging.

Use `ff`, not `git`, for anything that writes:
- commit: `ff commit -m \"…\"` — no add, no staging; the worktree is the change
- begin work: `ff start` · move: `ff switch <branch>` (parks and resumes dirty trees)
- undo the last operation, whole repo: `ff undo`
- discard edits to a file: `ff restore <path>`
- go back in time: `ff restore --all --at 2h`, or `ff op log` for ids then \
`ff restore <path> --at-op <id>`
- where you can go back to, one row per undo step: `ff history`
- fold a fix into an earlier commit: `ff absorb --into <rev>`
- take in base and remote: `ff sync` · send it out: `ff publish`
- anything else git does: `ff git <args…>` — snapshots, then runs git verbatim

Reading with git is fine. `ff status` and `ff log` say more than their git \
counterparts: the open change, and what the next rebase or push will do. \
`ff diff` sees the untracked files `git diff` does not.
";

// ---- the notice is a contract with the CLI ---------------------------------

/// `NOTICE` is prose that an agent reads as instructions, so it rots in a way
/// the compiler cannot see: a retired verb or a renamed flag still reads fine
/// and simply teaches the agent to fail. These guards make it fail here
/// instead — clap is the authority on what the notice is allowed to say.
#[cfg(test)]
mod notice {
    use clap::{CommandFactory, Parser};

    use super::NOTICE;
    use crate::cli::Cli;

    /// cli.rs as text, for the marker trail. Read as source rather than
    /// through clap because a comment is exactly what clap discards.
    const CLI_SRC: &str = include_str!("../cli.rs");
    const MARKER: &str = "// agent notice quotes this";

    /// Every `ff …` the notice spells, as argv. Placeholders (`<path>`,
    /// `"…"`) become a value, since the point is the grammar around them.
    fn quoted() -> Vec<Vec<String>> {
        NOTICE
            .split('`')
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
        let commands = quoted();
        assert!(
            commands.len() >= 8,
            "the notice stopped teaching verbs: {commands:?}"
        );
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
        for tokens in quoted() {
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
    /// exact token count to assert, so the budget is bytes: roughly 250
    /// tokens, and a rewrite that doubles it has to say so here.
    #[test]
    fn stays_within_its_budget() {
        assert!(
            NOTICE.len() <= 1_200,
            "the notice is {} bytes; trim it or raise the budget deliberately",
            NOTICE.len()
        );
    }
}
