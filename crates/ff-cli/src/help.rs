//! Every help page fufu prints, one `.txt` file per page under `help/`.
//!
//! The prose lives in those files rather than in `cli.rs` doc comments for
//! one mechanical reason: clap_derive joins a doc comment's lines into a
//! single paragraph and this build has no `wrap_help`, so a doc comment
//! prints as one very long line, while a `&'static str` is emitted line for
//! line. Nothing re-wraps them at runtime either — what a file holds is what
//! a terminal gets, at every width — which is why the widths below are the
//! page design rather than a tidiness.
//!
//! One file holds both halves clap prints, split at its `Examples:` line:
//! above it the long description that goes over `Usage:` (`long_about`),
//! below it the examples that go under the options (`after_long_help`). The
//! one-line `about` stays in `cli.rs`, where it is also the row in the
//! parent's command list.
//!
//! The `the_pages_are_formatted` test holds every file to one shape.
//! Column-0 prose in the description half is filled to 72 columns;
//! everything else — the indented lines, and the whole `Examples:` half,
//! which is a column-aligned table a reflow could only damage — is checked
//! at 80 and never rewritten. The check is an equality against the
//! formatter's own output, `cargo fmt --check` semantics, and
//! `FF_HELP_FMT=1 cargo test -p ff-cli --bins` rewrites the files instead of
//! reporting them.

/// The blank line where a page's two halves meet. `Examples:` appears once
/// in a file and only ever at column 0, which is what lets the split have no
/// special cases.
const SEAM: &str = "\n\nExamples:\n";

/// Where [`SEAM`] sits in a page. Const, so a file that lost its marker is a
/// compile error rather than a page that prints half of itself.
const fn seam(page: &str) -> usize {
    let (page, marker) = (page.as_bytes(), SEAM.as_bytes());
    let mut at = 0;
    while at + marker.len() <= page.len() {
        let mut i = 0;
        while i < marker.len() && page[at + i] == marker[i] {
            i += 1;
        }
        if i == marker.len() {
            return at;
        }
        at += 1;
    }
    panic!("a help page has no `Examples:` line");
}

/// Everything above the seam.
const fn description(page: &'static str) -> &'static str {
    page.split_at(seam(page)).0
}

/// The `Examples:` line and everything under it, less the file's trailing
/// newline: `long_about` and `after_long_help` are printed with their own
/// spacing, and a stray newline would land a blank line on the page.
const fn examples(page: &'static str) -> &'static str {
    let below = page.split_at(seam(page) + 2).1;
    below.split_at(below.len() - 1).0
}

/// The manifest: one row per page, naming the two consts `cli.rs` reads its
/// halves by and the file both come out of. The second column is always the
/// first with `_EXAMPLES` appended — `macro_rules!` cannot join identifiers,
/// so a row spells the name it declares rather than deriving it.
///
/// Braces rather than parens: rustfmt formats a parenthesized invocation as
/// a call and breaks the longer rows across five lines apiece, and a
/// manifest that cannot hold its columns is not one.
macro_rules! pages {
    ($($description:ident $examples:ident $file:literal)*) => {$(
        pub const $description: &str = description(include_str!($file));
        pub const $examples: &str = examples(include_str!($file));
    )*};
}

pages! {
    ROOT            ROOT_EXAMPLES            "help/root.txt"
    COLLIDE         COLLIDE_EXAMPLES         "help/collide.txt"
    STATUS          STATUS_EXAMPLES          "help/status.txt"
    LOG             LOG_EXAMPLES             "help/log.txt"
    DIFF            DIFF_EXAMPLES            "help/diff.txt"
    SHOW            SHOW_EXAMPLES            "help/show.txt"
    HISTORY         HISTORY_EXAMPLES         "help/history.txt"
    EVOLOG          EVOLOG_EXAMPLES          "help/evolog.txt"
    GIT             GIT_EXAMPLES             "help/git.txt"
    RESTORE         RESTORE_EXAMPLES         "help/restore.txt"
    TRIM            TRIM_EXAMPLES            "help/trim.txt"
    COMMIT          COMMIT_EXAMPLES          "help/commit.txt"
    SWITCH          SWITCH_EXAMPLES          "help/switch.txt"
    UNDO            UNDO_EXAMPLES            "help/undo.txt"
    START           START_EXAMPLES           "help/start.txt"
    DESCRIBE        DESCRIBE_EXAMPLES        "help/describe.txt"
    ABSORB          ABSORB_EXAMPLES          "help/absorb.txt"
    LIFT            LIFT_EXAMPLES            "help/lift.txt"
    RESTACK         RESTACK_EXAMPLES         "help/restack.txt"
    SYNC            SYNC_EXAMPLES            "help/sync.txt"
    PUBLISH         PUBLISH_EXAMPLES         "help/publish.txt"
    REMOTE          REMOTE_EXAMPLES          "help/remote.txt"
    INIT            INIT_EXAMPLES            "help/init.txt"
    CLONE           CLONE_EXAMPLES           "help/clone.txt"
    EDIT            EDIT_EXAMPLES            "help/edit.txt"
    DONE            DONE_EXAMPLES            "help/done.txt"
    RESOLVE         RESOLVE_EXAMPLES         "help/resolve.txt"
    BRANCH          BRANCH_EXAMPLES          "help/branch.txt"
    WORKTREE        WORKTREE_EXAMPLES        "help/worktree.txt"
    BRANCH_LIST     BRANCH_LIST_EXAMPLES     "help/branch-list.txt"
    WORKTREE_LIST   WORKTREE_LIST_EXAMPLES   "help/worktree-list.txt"
    WORKTREE_ADD    WORKTREE_ADD_EXAMPLES    "help/worktree-add.txt"
    WORKTREE_REMOVE WORKTREE_REMOVE_EXAMPLES "help/worktree-remove.txt"
    BRANCH_DELETE   BRANCH_DELETE_EXAMPLES   "help/branch-delete.txt"
    HOOK            HOOK_EXAMPLES            "help/hook.txt"
    UNHOOK          UNHOOK_EXAMPLES          "help/unhook.txt"
    TRIGGER         TRIGGER_EXAMPLES         "help/trigger.txt"
    WATCH           WATCH_EXAMPLES           "help/watch.txt"
    CONFIG          CONFIG_EXAMPLES          "help/config.txt"
    DOCTOR          DOCTOR_EXAMPLES          "help/doctor.txt"
    VERSION         VERSION_EXAMPLES         "help/version.txt"
    UPDATE          UPDATE_EXAMPLES          "help/update.txt"
    REDO            REDO_EXAMPLES            "help/redo.txt"
    OP              OP_EXAMPLES              "help/op.txt"
    OP_LOG          OP_LOG_EXAMPLES          "help/op-log.txt"
    OP_SHOW         OP_SHOW_EXAMPLES         "help/op-show.txt"
    OP_DIFF         OP_DIFF_EXAMPLES         "help/op-diff.txt"
    OP_RESTORE      OP_RESTORE_EXAMPLES      "help/op-restore.txt"
    OP_REVERT       OP_REVERT_EXAMPLES       "help/op-revert.txt"
}

// ---------------------------------------------------------------------------
// The root page's command list.
//
// clap cannot group subcommands: `subcommand_help_heading` renames the single
// `Commands:` header, and `help_heading` is an `Arg` property a subcommand
// never consults. So fufu renders the list itself and hands it to clap as a
// `help_template` on the root command only.
//
// The template writes `{options}` rather than `{all-args}`, and that is what
// keeps clap's own flat list from rendering at all — no subcommand needs
// hiding, so suggestions, `ff help <cmd>` and dispatch are untouched. The
// `Options:` heading `{all-args}` would have written is written by hand
// instead, in clap's own header style.
// ---------------------------------------------------------------------------

/// One command in the list.
pub struct Row {
    pub name: &'static str,
    /// Shown by `ff -h`; the rest wait for `ff --help`.
    pub common: bool,
}

/// One heading, and the commands under it.
pub struct Group {
    pub heading: &'static str,
    pub commands: &'static [Row],
}

/// Shorthand for the table below: `c` is a common verb, `r` is the rest.
const fn c(name: &'static str) -> Row {
    Row { name, common: true }
}
const fn r(name: &'static str) -> Row {
    Row {
        name,
        common: false,
    }
}

/// Every command, grouped. Four of the headings are `git help`'s own words,
/// because git already solved this page and a reader who knows one should
/// not have to learn the other; the fufu-only groups are written in the same
/// register.
///
/// Three placements are deliberate. `commit` sits with the current change
/// rather than under "grow, mark and tweak", because in fufu the working
/// tree *is* the change. `restore` sits there too, where git has it. And
/// `map` heads "examine" rather than taking a line of its own, since bare
/// `ff` is taught two paragraphs into [`ROOT`].
///
/// clap's generated `help` subcommand is not a row: it exists only after
/// `Command::build()`, which [`root_template`] deliberately does not call,
/// and [`ROOT_EXAMPLES`] already teaches `ff help <command>`.
pub const GROUPS: &[Group] = &[
    Group {
        heading: "start a working area",
        commands: &[c("init"), c("clone"), r("worktree")],
    },
    Group {
        heading: "work on the current change",
        commands: &[
            c("status"),
            c("diff"),
            r("restore"),
            c("commit"),
            c("describe"),
        ],
    },
    Group {
        heading: "examine the history and state",
        commands: &[
            r("map"),
            c("log"),
            c("show"),
            r("evolog"),
            r("history"),
            r("collide"),
        ],
    },
    Group {
        heading: "grow, mark and tweak your common history",
        commands: &[
            c("start"),
            c("switch"),
            c("branch"),
            r("absorb"),
            r("lift"),
            r("restack"),
            r("edit"),
            r("done"),
            r("resolve"),
        ],
    },
    Group {
        heading: "collaborate",
        commands: &[c("sync"), c("publish"), r("remote")],
    },
    Group {
        heading: "go back",
        commands: &[c("undo"), r("redo"), r("op"), r("trim")],
    },
    Group {
        heading: "wire it in, and check on it",
        commands: &[
            r("hook"),
            r("unhook"),
            r("trigger"),
            r("watch"),
            r("config"),
            r("doctor"),
        ],
    },
    Group {
        heading: "fufu itself",
        commands: &[r("git"), r("explain"), r("version"), r("update")],
    },
];

/// The `help_template` for the root page: clap's own frame, with the grouped
/// list where its flat `Commands:` section would have been.
///
/// `long` is the `-h`/`--help` spelling, which clap decides for itself and
/// does not expose to the template — main reads it from argv instead. Short
/// help shows only the common verbs, and closes with a line naming the long
/// spelling.
///
/// Styles are painted unconditionally. clap prints help through an
/// `anstream::AutoStream`, which strips the escapes when color is off, so
/// this block colors exactly when clap's own `Usage:` and `Options:` do and
/// there is no second color decision to keep in step.
pub fn root_template(long: bool) -> String {
    use clap::CommandFactory;
    use std::fmt::Write as _;

    // Not built: `Command::build()` is the frame the stack budget guards
    // (cli.rs's `the_command_tree_fits_a_small_stack`), and the only row it
    // would add is clap's own `help`.
    let root = crate::cli::Cli::command();
    let styles = root.get_styles();
    let header = *styles.get_header();
    let literal = *styles.get_literal();
    let context = *styles.get_context();
    let context_value = *styles.get_context_value();

    // What clap would have printed for each row, minus the name: the `about`
    // line plus any visible alias, spelled the way clap spells it. Hidden
    // subcommands drop out here — which is exactly what keeps the eight
    // foreign git words off the page.
    let live: Vec<(&str, String)> = root
        .get_subcommands()
        .filter(|sc| !sc.is_hide_set())
        .map(|sc| {
            let mut about = sc.get_about().map(ToString::to_string).unwrap_or_default();
            let aliases: Vec<&str> = sc.get_visible_aliases().collect();
            if !aliases.is_empty() {
                let plural = if aliases.len() == 1 { "" } else { "es" };
                let names = aliases
                    .iter()
                    .map(|a| format!("{context_value}{a}{context_value:#}"))
                    .collect::<Vec<_>>()
                    .join(&format!("{context}, {context:#}"));
                let _ = write!(
                    about,
                    " {context}[alias{plural}: {context:#}{names}{context}]{context:#}"
                );
            }
            (sc.get_name(), about)
        })
        .collect();
    let about_of = |name: &str| {
        live.iter()
            .find(|(live_name, _)| *live_name == name)
            .map(|(_, about)| about.as_str())
    };

    // One name column for the whole page, not one per heading: the headings
    // are what group the rows, and a column that moved under each of them
    // would undo that.
    let width = GROUPS
        .iter()
        .flat_map(|group| group.commands)
        .filter(|row| (long || row.common) && about_of(row.name).is_some())
        .map(|row| row.name.chars().count())
        .max()
        .unwrap_or(0);

    let mut block = String::new();
    for group in GROUPS {
        let rows: Vec<&Row> = group
            .commands
            .iter()
            .filter(|row| (long || row.common) && about_of(row.name).is_some())
            .collect();
        if rows.is_empty() {
            continue;
        }
        if !block.is_empty() {
            block.push('\n');
        }
        let _ = writeln!(block, "{header}{}:{header:#}", group.heading);
        for row in rows {
            let name = crate::render::col(row.name, width, literal, true);
            let about = about_of(row.name).unwrap_or_default();
            let _ = writeln!(block, "  {name}  {about}");
        }
    }

    let mut template = String::from("{before-help}{about-with-newline}\n{usage-heading} {usage}\n");
    let _ = write!(template, "\n{block}");
    if !long {
        let dim = anstyle::Style::new().dimmed();
        let _ = writeln!(
            template,
            "\n  {dim}see `ff --help` for every command{dim:#}"
        );
    }
    let _ = write!(template, "\n{header}Options:{header:#}\n");
    template.push_str("{options}{after-help}");
    template
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is a second list of the command tree, so it has to be the
    /// same list — the guard `rawgit::TABLE` already writes for its own.
    #[test]
    fn every_live_command_is_grouped_exactly_once() {
        use clap::CommandFactory;

        let root = crate::cli::Cli::command();
        let live: Vec<&str> = root
            .get_subcommands()
            .filter(|sc| !sc.is_hide_set())
            .map(|sc| sc.get_name())
            .collect();

        let listed: Vec<&str> = GROUPS
            .iter()
            .flat_map(|group| group.commands)
            .map(|row| row.name)
            .collect();

        for name in &live {
            let seen = listed.iter().filter(|listed| *listed == name).count();
            assert_eq!(seen, 1, "{name} appears in {seen} groups, want exactly 1");
        }
        for name in &listed {
            assert!(
                live.contains(name),
                "{name} is grouped but is not live, non-hidden surface"
            );
        }
    }

    /// clap echoes an unknown `{tag}` back verbatim, so a brace anywhere in
    /// the pre-rendered block would corrupt the page it is embedded in.
    #[test]
    fn nothing_on_the_page_carries_a_brace() {
        use clap::CommandFactory;

        let root = crate::cli::Cli::command();
        for sc in root.get_subcommands().filter(|sc| !sc.is_hide_set()) {
            let about = sc.get_about().map(ToString::to_string).unwrap_or_default();
            assert!(
                !about.contains(['{', '}']),
                "{}'s about carries a brace: {about:?}",
                sc.get_name()
            );
        }
        for group in GROUPS {
            assert!(
                !group.heading.contains(['{', '}']),
                "the heading {:?} carries a brace",
                group.heading
            );
            for row in group.commands {
                assert!(
                    !row.name.contains(['{', '}']),
                    "the row {:?} carries a brace",
                    row.name
                );
            }
        }
    }

    /// Column-0 prose in a page's description half is filled to this.
    const FILL: usize = 72;

    /// What nothing may exceed: the indented lines, and the aligned rows of
    /// the `Examples:` half. Neither can be reflowed — a table row has no
    /// line breaks to move — so a row over this is reworded, not rewrapped.
    const CAP: usize = 80;

    /// The width each line of a page is held to.
    fn caps(page: &str) -> Vec<usize> {
        let seam = page.find(SEAM).unwrap_or(page.len());
        let mut at = 0;
        let mut caps = Vec::new();
        for line in page.lines() {
            let prose = at < seam && !line.starts_with(' ');
            caps.push(if prose { FILL } else { CAP });
            at += line.len() + 1;
        }
        caps
    }

    /// Greedy fill at [`FILL`], counting characters rather than bytes: the
    /// prose is full of em dashes, and the widest line in these files is 71
    /// characters and 75 bytes. A blank line is kept exactly, and an
    /// indented line ends the paragraph above it and is never reflowed —
    /// `restore`'s three flag rows are the ones that depend on that.
    fn fill(half: &str) -> String {
        fn flush(out: &mut String, words: &mut Vec<&str>) {
            let mut line = String::new();
            for word in words.drain(..) {
                let too_wide = line.chars().count() + 1 + word.chars().count() > FILL;
                if !line.is_empty() && too_wide {
                    out.push_str(&line);
                    out.push('\n');
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
            if !line.is_empty() {
                out.push_str(&line);
                out.push('\n');
            }
        }

        let mut out = String::new();
        let mut words: Vec<&str> = Vec::new();
        for line in half.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with(' ') {
                flush(&mut out, &mut words);
                out.push_str(line);
                out.push('\n');
            } else {
                words.extend(line.split_whitespace());
            }
        }
        flush(&mut out, &mut words);
        out.truncate(out.trim_end().len());
        out
    }

    /// What a page file should hold, byte for byte.
    fn formatted(page: &str) -> String {
        let seam = page.find(SEAM).expect("a page has an `Examples:` line");
        let mut out = fill(&page[..seam]);
        out.push_str("\n\n");
        for line in page[seam + 2..].trim_end().lines() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    /// The pages are one width, and stay one width. The check is an equality
    /// against the formatter's own output rather than "no line is too long",
    /// which is what makes it hold over time; greedy fill is idempotent, so
    /// a formatted file is a fixed point.
    #[test]
    fn the_pages_are_formatted() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/help");
        let rewrite = std::env::var_os("FF_HELP_FMT").is_some();

        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("the pages live beside this module")
            .map(|entry| entry.expect("a readable directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
            .collect();
        paths.sort();

        let mut wrote = 0;
        let mut unformatted: Vec<String> = Vec::new();
        let mut overwide: Vec<String> = Vec::new();

        for path in &paths {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let src = std::fs::read_to_string(path).expect("a page is UTF-8");
            let want = formatted(&src);

            if src != want {
                if rewrite {
                    std::fs::write(path, &want).expect("a writable page");
                    wrote += 1;
                } else {
                    let (have, mine): (Vec<&str>, Vec<&str>) =
                        (src.lines().collect(), want.lines().collect());
                    let at = (0..have.len().max(mine.len()))
                        .find(|i| have.get(*i) != mine.get(*i))
                        .unwrap_or(0);
                    let line = have.get(at).copied().unwrap_or_default();
                    let wide = line.chars().count();
                    let cap = caps(&src).get(at).copied().unwrap_or(CAP);
                    unformatted.push(if wide > cap {
                        format!("help/{name}:{} is {wide} columns, want <= {cap}", at + 1)
                    } else {
                        format!(
                            "help/{name}:{} is not the fill\n    have: {line}\n    want: {}",
                            at + 1,
                            mine.get(at).copied().unwrap_or_default()
                        )
                    });
                }
            }

            // Against the formatted text, so the only widths left are the
            // ones a reflow cannot reach.
            for ((at, line), cap) in want.lines().enumerate().zip(caps(&want)) {
                let wide = line.chars().count();
                if wide > cap {
                    overwide.push(format!(
                        "help/{name}:{} is {wide} columns, want <= {cap} — reword it, \
                         the fill cannot reach this line",
                        at + 1
                    ));
                }
                if line.contains('\t') {
                    overwide.push(format!("help/{name}:{} has a tab", at + 1));
                }
            }
        }

        assert!(
            paths.len() >= 40,
            "the walk found {} pages under {}, so it is checking nothing",
            paths.len(),
            dir.display()
        );

        if rewrite {
            println!("rewrapped {wrote} files");
        }

        let mut report = String::new();
        for finding in unformatted.iter().chain(&overwide) {
            report.push_str(finding);
            report.push('\n');
        }
        if !unformatted.is_empty() {
            report.push_str("  run: FF_HELP_FMT=1 cargo test -p ff-cli --bins\n");
        }
        assert!(report.is_empty(), "\n{report}");
    }
}
