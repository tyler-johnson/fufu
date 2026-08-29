//! Every help page fufu prints, one markdown file per page under `help/`.
//!
//! Markdown is the source of truth: the docs site's CLI reference emits
//! these files verbatim, and the terminal gets a best-effort rendering
//! through [`term`] — a paragraph is one line in the file and is filled to
//! 72 columns at render time, `## Examples` prints as `Examples:`, and a
//! fenced block loses its fence lines and gains a two-space indent. Nothing
//! else is translated: backticks and emphasis print as typed, which is how
//! these pages already read.
//!
//! The prose lives in files rather than in `cli.rs` doc comments for one
//! mechanical reason: clap_derive joins a doc comment's lines into a single
//! paragraph, while a rendered string is emitted line for line.
//!
//! One file holds both halves clap prints, split at its `## Examples`
//! heading: above it the long description that goes over `Usage:`
//! (`long_about`), below it the examples that go under the options
//! (`after_long_help`). The one-line `about` stays in `cli.rs`, where it is
//! also the row in the parent's command list.
//!
//! The `the_pages_are_formatted` test holds every file to one shape: one
//! line per paragraph, balanced fences, exactly one `## Examples`, and no
//! line the renderer would print wider than 80 columns. The check is an
//! equality against the formatter's own output, `cargo fmt --check`
//! semantics, and `FF_HELP_FMT=1 cargo test -p ff-cli --bins` rewrites the
//! files (re-joining hand-wrapped paragraphs) instead of reporting them.

/// The blank line where a page's two halves meet. `## Examples` appears once
/// in a file and only ever at column 0, which is what lets the split have no
/// special cases.
pub(crate) const SEAM: &str = "\n\n## Examples\n";

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
    panic!("a help page has no `## Examples` heading");
}

/// Everything above the seam.
const fn description(page: &'static str) -> &'static str {
    page.split_at(seam(page)).0
}

/// The `## Examples` heading and everything under it, less the file's
/// trailing newline: `long_about` and `after_long_help` are printed with
/// their own spacing, and a stray newline would land a blank line on the
/// page.
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
    ROOT            ROOT_EXAMPLES            "help/root.md"
    COLLIDE         COLLIDE_EXAMPLES         "help/collide.md"
    STATUS          STATUS_EXAMPLES          "help/status.md"
    LOG             LOG_EXAMPLES             "help/log.md"
    DIFF            DIFF_EXAMPLES            "help/diff.md"
    SHOW            SHOW_EXAMPLES            "help/show.md"
    HISTORY         HISTORY_EXAMPLES         "help/history.md"
    EVOLOG          EVOLOG_EXAMPLES          "help/evolog.md"
    GIT             GIT_EXAMPLES             "help/git.md"
    RESTORE         RESTORE_EXAMPLES         "help/restore.md"
    TRIM            TRIM_EXAMPLES            "help/trim.md"
    COMMIT          COMMIT_EXAMPLES          "help/commit.md"
    SWITCH          SWITCH_EXAMPLES          "help/switch.md"
    UNDO            UNDO_EXAMPLES            "help/undo.md"
    START           START_EXAMPLES           "help/start.md"
    DESCRIBE        DESCRIBE_EXAMPLES        "help/describe.md"
    ABSORB          ABSORB_EXAMPLES          "help/absorb.md"
    LIFT            LIFT_EXAMPLES            "help/lift.md"
    RESTACK         RESTACK_EXAMPLES         "help/restack.md"
    SYNC            SYNC_EXAMPLES            "help/sync.md"
    PUBLISH         PUBLISH_EXAMPLES         "help/publish.md"
    REMOTE          REMOTE_EXAMPLES          "help/remote.md"
    INIT            INIT_EXAMPLES            "help/init.md"
    CLONE           CLONE_EXAMPLES           "help/clone.md"
    EDIT            EDIT_EXAMPLES            "help/edit.md"
    DONE            DONE_EXAMPLES            "help/done.md"
    RESOLVE         RESOLVE_EXAMPLES         "help/resolve.md"
    BRANCH          BRANCH_EXAMPLES          "help/branch.md"
    WORKTREE        WORKTREE_EXAMPLES        "help/worktree.md"
    BRANCH_LIST     BRANCH_LIST_EXAMPLES     "help/branch-list.md"
    WORKTREE_LIST   WORKTREE_LIST_EXAMPLES   "help/worktree-list.md"
    WORKTREE_ADD    WORKTREE_ADD_EXAMPLES    "help/worktree-add.md"
    WORKTREE_REMOVE WORKTREE_REMOVE_EXAMPLES "help/worktree-remove.md"
    BRANCH_DELETE   BRANCH_DELETE_EXAMPLES   "help/branch-delete.md"
    HOOK            HOOK_EXAMPLES            "help/hook.md"
    UNHOOK          UNHOOK_EXAMPLES          "help/unhook.md"
    TRIGGER         TRIGGER_EXAMPLES         "help/trigger.md"
    WATCH           WATCH_EXAMPLES           "help/watch.md"
    CONFIG          CONFIG_EXAMPLES          "help/config.md"
    DOCTOR          DOCTOR_EXAMPLES          "help/doctor.md"
    VERSION         VERSION_EXAMPLES         "help/version.md"
    UPDATE          UPDATE_EXAMPLES          "help/update.md"
    REDO            REDO_EXAMPLES            "help/redo.md"
    OP              OP_EXAMPLES              "help/op.md"
    OP_LOG          OP_LOG_EXAMPLES          "help/op-log.md"
    OP_SHOW         OP_SHOW_EXAMPLES         "help/op-show.md"
    OP_DIFF         OP_DIFF_EXAMPLES         "help/op-diff.md"
    OP_RESTORE      OP_RESTORE_EXAMPLES      "help/op-restore.md"
    OP_REVERT       OP_REVERT_EXAMPLES       "help/op-revert.md"
}

// ---------------------------------------------------------------------------
// The terminal rendering.
//
// clap is built without `wrap_help`, so whatever these functions return is
// emitted line for line, at every width. The 72-column fill is the page
// design, not a terminal measurement — the same width the files themselves
// held before markdown became the source of truth.
// ---------------------------------------------------------------------------

/// Column-0 prose is filled to this at render time.
const FILL: usize = 72;

/// Render one markdown half for the terminal.
///
/// Three translations, everything else verbatim: a column-0 paragraph (one
/// line in the file) is filled to [`FILL`] columns; `## Examples` prints as
/// `Examples:`; a fenced block loses its fence lines and its content gains a
/// two-space indent. The blank line markdown wants between a lead-in
/// paragraph and a fence — or after the heading — is not printed, so a table
/// hugs the colon line that introduces it, the way these pages always read.
pub fn term(md: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    // A blank seen but not yet printed: it is dropped if the next line opens
    // a fence, and printed the moment anything else follows.
    let mut blank_pending = false;
    // The blank under `## Examples`, dropped outright.
    let mut swallow_blank = false;
    for line in md.lines() {
        if in_fence {
            if line.starts_with("```") {
                in_fence = false;
            } else if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if line.starts_with("```") {
            blank_pending = false;
            swallow_blank = false;
            in_fence = true;
            continue;
        }
        if line.is_empty() {
            if swallow_blank {
                swallow_blank = false;
            } else {
                blank_pending = true;
            }
            continue;
        }
        if blank_pending {
            out.push('\n');
            blank_pending = false;
        }
        swallow_blank = false;
        if line == "## Examples" {
            out.push_str("Examples:\n");
            swallow_blank = true;
        } else if line.starts_with(' ') {
            // Markdown holds no indented prose — the formatter test refuses
            // it — but a renderer never eats a line it does not understand.
            out.push_str(line);
            out.push('\n');
        } else {
            fill(&mut out, line);
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// [`term`] for the `## Examples` half — the same rendering, named so a
/// `cli.rs` attachment reads as the pair it is: `term(X)` over the usage,
/// `term_examples(X_EXAMPLES)` under the options.
pub fn term_examples(md: &str) -> String {
    term(md)
}

/// Greedy fill at [`FILL`], counting characters rather than bytes: the prose
/// is full of em dashes. A word is never broken, so a token wider than the
/// fill stands alone on its line.
fn fill(out: &mut String, paragraph: &str) {
    let mut width = 0;
    for word in paragraph.split_whitespace() {
        let wide = word.chars().count();
        if width > 0 && width + 1 + wide > FILL {
            out.push('\n');
            width = 0;
        }
        if width > 0 {
            out.push(' ');
            width += 1;
        }
        out.push_str(word);
        width += wide;
    }
    if width > 0 {
        out.push('\n');
    }
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

    /// What nothing the renderer prints may exceed. The prose fill lands at
    /// [`FILL`] on its own; what this cap actually holds are the fenced
    /// rows, which have no line breaks to move — a row over it is reworded,
    /// not rewrapped.
    const CAP: usize = 80;

    /// What a page file should hold, byte for byte: one line per paragraph.
    /// Consecutive column-0 prose lines are joined — that is the whole
    /// formatter, and it is what `FF_HELP_FMT=1` uses to unwrap a paragraph
    /// somebody hand-wrapped. Fences, their content, headings, and blank
    /// lines pass through with trailing whitespace trimmed.
    fn formatted(page: &str) -> String {
        let mut out = String::new();
        let mut para: Vec<&str> = Vec::new();
        let mut in_fence = false;
        let flush = |out: &mut String, para: &mut Vec<&str>| {
            if !para.is_empty() {
                out.push_str(&para.join(" "));
                out.push('\n');
                para.clear();
            }
        };
        for line in page.lines() {
            let line = line.trim_end();
            if in_fence || line.starts_with("```") {
                if line.starts_with("```") {
                    in_fence = !in_fence;
                }
                flush(&mut out, &mut para);
                out.push_str(line);
                out.push('\n');
            } else if line.is_empty() || line.starts_with(' ') || line.starts_with('#') {
                flush(&mut out, &mut para);
                out.push_str(line);
                out.push('\n');
            } else {
                para.push(line);
            }
        }
        flush(&mut out, &mut para);
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    }

    /// The shape a page cannot be rewritten into: findings the formatter has
    /// no move for, reported beside the equality check.
    fn malformed(name: &str, src: &str) -> Vec<String> {
        let mut findings = Vec::new();
        let mut in_fence = false;
        let mut headings = 0;
        for (at, line) in src.lines().enumerate() {
            let row = at + 1;
            if line.starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if line.contains('\t') {
                findings.push(format!("help/{name}:{row} has a tab"));
            }
            if in_fence {
                continue;
            }
            if line == "## Examples" {
                headings += 1;
            } else if line.starts_with('#') {
                findings.push(format!(
                    "help/{name}:{row} is a heading; `## Examples` is the only one a page holds"
                ));
            }
            if line.starts_with(' ') && !line.trim().is_empty() {
                findings.push(format!(
                    "help/{name}:{row} is indented outside a fence — put it in a fenced block"
                ));
            }
        }
        if in_fence {
            findings.push(format!("help/{name} has an unclosed fence"));
        }
        if headings != 1 {
            findings.push(format!(
                "help/{name} has {headings} `## Examples` headings, want exactly 1"
            ));
        }
        findings
    }

    /// The pages are markdown, and stay renderable: one line per paragraph
    /// (the equality check against [`formatted`], `cargo fmt --check`
    /// semantics), the structural invariants [`malformed`] names, and
    /// nothing [`term`] prints wider than [`CAP`] — the fenced rows are the
    /// lines the fill cannot reach, so a wide one is reworded, not
    /// rewrapped.
    #[test]
    fn the_pages_are_formatted() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/help");
        let rewrite = std::env::var_os("FF_HELP_FMT").is_some();

        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("the pages live beside this module")
            .map(|entry| entry.expect("a readable directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        paths.sort();

        let mut wrote = 0;
        let mut unformatted: Vec<String> = Vec::new();
        let mut broken: Vec<String> = Vec::new();

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
                    unformatted.push(format!(
                        "help/{name}:{} is not one line per paragraph\n    have: {}\n    want: {}",
                        at + 1,
                        have.get(at).copied().unwrap_or_default(),
                        mine.get(at).copied().unwrap_or_default()
                    ));
                }
            }

            broken.extend(malformed(&name, &want));

            // What the terminal will actually print, held to the cap. The
            // description fill lands under it by construction; the fenced
            // rows are the lines only a rewording can narrow.
            let seam = want.find(SEAM).map(|at| at + 2).unwrap_or(want.len());
            for half in [&want[..seam.min(want.len())], &want[seam.min(want.len())..]] {
                for line in term(half).lines() {
                    let wide = line.chars().count();
                    if wide > CAP {
                        broken.push(format!(
                            "help/{name} renders {wide} columns, want <= {CAP} — reword it, \
                             the fill cannot reach this line:\n    {line}"
                        ));
                    }
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
            println!("rewrote {wrote} files");
        }

        let mut report = String::new();
        for finding in unformatted.iter().chain(&broken) {
            report.push_str(finding);
            report.push('\n');
        }
        if !unformatted.is_empty() {
            report.push_str("  run: FF_HELP_FMT=1 cargo test -p ff-cli --bins\n");
        }
        assert!(report.is_empty(), "\n{report}");
    }
}
