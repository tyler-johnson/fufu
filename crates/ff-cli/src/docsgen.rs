//! The CLI reference generator: one markdown page per live command under
//! `docs/reference/cli/`, plus the index that groups them the way the root
//! help page does.
//!
//! It is a test rather than a bin on the repo's codegen idiom (`help.rs`,
//! `explain.rs`, `revset/resolve.rs`): the walk regenerates every page and
//! asserts the checked-in files match byte for byte, so the reference cannot
//! drift from `--help` — CI fails instead. `FF_DOCS_GEN=1 cargo test -p
//! ff-cli --bins docs` rewrites the files. A test is also the only place
//! this can live: `Cli::command()` and `help::GROUPS` are crate-private.
//!
//! A page is the help file's markdown emitted verbatim — the description
//! above a fenced `## Usage` block holding what clap prints for the verb,
//! the `## Examples` section below it. The prose is docs-grade because it is
//! the same prose `ff help <verb>` renders; nothing is written twice. The one
//! transformation is a link: the first mention of every other verb on a page
//! points at that verb's page, the convention the hand-written docs keep.
//!
//! `docs/reference/config.md` gets the same treatment in miniature: the
//! settings registry in `cmd/config.rs` renders into a marked region of that
//! page — `<!-- registry:begin -->` through `<!-- registry:end -->` — under
//! the same byte-equality and `FF_DOCS_GEN=1` contract, with the prose
//! around the markers hand-written and untouched. `docs/reference/errors.md`
//! is the third region: the error id registry in `explain.rs`, one table row
//! per id with the exit code it carries.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use clap::CommandFactory;

use crate::help;

/// One generated page: its file name under `docs/reference/cli/`, and what
/// it holds.
struct Page {
    file: String,
    content: String,
}

/// Where the generated pages live: `docs/reference/cli/` at the repo root,
/// reached from this crate the way the workspace lays it out.
fn dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/cli")
}

/// The help file backing a command, by the same name the `pages!` manifest
/// uses: `op log` reads `op-log.md`, and `map` reads the root page it shares.
fn source(path: &str) -> Option<String> {
    let file = if path == "map" {
        "root.md".to_string()
    } else {
        format!("{}.md", path.replace(' ', "-"))
    };
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/help")
        .join(file);
    std::fs::read_to_string(file).ok()
}

/// What clap prints for one command, usage line through the options — the
/// default long-help page minus the two halves the markdown already carries.
fn usage(cmd: &clap::Command) -> String {
    let mut cmd = cmd
        .clone()
        .help_template("{usage-heading} {usage}\n\n{all-args}");
    cmd.render_long_help().to_string().trim_end().to_string()
}

/// The first mention of every other verb on a page becomes a link to that
/// verb's page: `` `ff op log` `` reads `[`ff op log`](op-log.md)` once and
/// bare after, which is the convention the hand-written pages keep. The
/// page's own verb stays bare, fenced blocks and headings are left alone, and
/// a span naming no page (`ff help`, an extension) is not touched. Paths are
/// matched longest first, so `ff op log` reaches its own page rather than
/// `ff op`'s.
fn linkify(md: &str, this: &str, verbs: &[String]) -> String {
    let mut done: HashSet<&str> = HashSet::new();
    done.insert(this);
    let mut out = String::with_capacity(md.len());
    let mut fenced = false;
    for line in md.split_inclusive('\n') {
        if line.starts_with("```") {
            fenced = !fenced;
        }
        if fenced || line.starts_with('#') {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("`ff ") {
            let Some(len) = rest[start + 1..].find('`') else {
                break;
            };
            let span = &rest[start..start + len + 2];
            let words: Vec<&str> = span[1..span.len() - 1].split_whitespace().skip(1).collect();
            let hit = verbs
                .iter()
                .filter(|verb| {
                    let want: Vec<&str> = verb.split(' ').collect();
                    words.len() >= want.len() && words[..want.len()] == want[..]
                })
                .max_by_key(|verb| verb.len());
            out.push_str(&rest[..start]);
            match hit {
                Some(verb) if !done.contains(verb.as_str()) => {
                    done.insert(verb);
                    let _ = write!(out, "[{span}]({}.md)", verb.replace(' ', "-"));
                }
                _ => out.push_str(span),
            }
            rest = &rest[start + len + 2..];
        }
        out.push_str(rest);
    }
    out
}

/// A page's `### ` subheadings become `## ` on the web copy.
///
/// In the terminal they are subsections of one flat page. Here the page has
/// gained an `# ff <verb>` title above them and `## Usage` and `## Examples`
/// beside them, so the honest level is the one those two already sit at —
/// and a jump from `#` straight to `###` would leave the table of contents
/// nesting them under a level that is not there.
fn promote(description: &str) -> String {
    let mut out = String::with_capacity(description.len());
    let mut fenced = false;
    for line in description.split_inclusive('\n') {
        if line.starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && line.starts_with("### ") {
            out.push_str(&line[1..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// One command's page: title, the markdown description with the other verbs
/// linked on first mention, the fenced usage block, and the `## Examples`
/// section verbatim.
fn page(path: &str, cmd: &clap::Command, verbs: &[String]) -> Page {
    let mut content = format!("# ff {path}\n\n");
    match source(path) {
        Some(src) => {
            let src = linkify(&src, path, verbs);
            let seam = src
                .find(help::SEAM)
                .unwrap_or_else(|| panic!("help/{path}: no `## Examples` heading"));
            let _ = write!(
                content,
                "{}\n\n## Usage\n\n```\n{}\n```\n\n{}",
                promote(&src[..seam]),
                usage(cmd),
                src[seam + 2..].trim_end()
            );
        }
        // The one verb with no page file (`explain`) documents itself with
        // its derive `about`, the same text `ff help explain` falls back to.
        None => {
            let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
            let _ = write!(
                content,
                "{}.\n\n## Usage\n\n```\n{}\n```",
                about,
                usage(cmd)
            );
        }
    }
    content.push('\n');
    Page {
        file: format!("{}.md", path.replace(' ', "-")),
        content,
    }
}

/// The non-hidden subcommands of a family, in declaration order. The
/// external-subcommand catch-all (`branch`'s `Other`) is not a clap
/// subcommand, so it never appears; clap's own `help` exists only because
/// the tree is built, and it is not a page.
fn family(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|sc| !sc.is_hide_set() && sc.get_name() != "help")
        .collect()
}

/// Every page, in the order the index lists them: `GROUPS` × the live
/// command tree, each family verb followed by its subcommand pages.
fn pages() -> Vec<Page> {
    let mut root = crate::cli::Cli::command();
    // Propagates `bin_name` down the tree, so a subcommand's usage line
    // reads `Usage: ff op log` rather than starting at its own name.
    root.build();

    let mut index = String::from(
        "# CLI reference\n\n\
         Every command, grouped the way `ff --help` groups them. Each page is the same text \
         `ff help <command>` prints, with clap's usage block between the two halves. This \
         directory is generated from `crates/ff-cli/src/help/` by a test — edit there, \
         then `make docs-gen`.\n",
    );
    let mut out = Vec::new();

    // Every page's verb path, known before any page is written, so a page
    // can link to one the walk has not reached yet.
    let mut verbs: Vec<String> = Vec::new();
    for group in help::GROUPS {
        for row in group.commands {
            let cmd = root
                .find_subcommand(row.name)
                .unwrap_or_else(|| panic!("{} is grouped but not live", row.name));
            verbs.push(row.name.to_string());
            for sub in family(cmd) {
                verbs.push(format!("{} {}", row.name, sub.get_name()));
            }
        }
    }

    for group in help::GROUPS {
        let _ = write!(index, "\n## {}\n\n", group.heading);
        for row in group.commands {
            let cmd = root
                .find_subcommand(row.name)
                .unwrap_or_else(|| panic!("{} is grouped but not live", row.name));
            let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
            let _ = writeln!(index, "- [`ff {0}`]({0}.md) — {1}", row.name, about);
            out.push(page(row.name, cmd, &verbs));
            for sub in family(cmd) {
                let path = format!("{} {}", row.name, sub.get_name());
                let about = sub.get_about().map(ToString::to_string).unwrap_or_default();
                let _ = writeln!(
                    index,
                    "    - [`ff {path}`]({}.md) — {about}",
                    path.replace(' ', "-")
                );
                out.push(page(&path, sub, &verbs));
            }
        }
    }

    out.push(Page {
        file: "index.md".to_string(),
        content: index,
    });
    out
}

/// The checked-in reference equals regeneration, byte for byte, and holds
/// nothing else. `FF_DOCS_GEN=1` rewrites the directory instead of
/// reporting it — strays included, because the generator owns it whole.
#[test]
fn the_reference_is_generated() {
    let dir = dir();
    let rewrite = std::env::var_os("FF_DOCS_GEN").is_some();
    let pages = pages();

    assert!(
        pages.len() > 40,
        "the walk produced {} pages, so it is generating nothing",
        pages.len()
    );

    let mut findings: Vec<String> = Vec::new();
    let mut wrote = 0;

    for page in &pages {
        let path = dir.join(&page.file);
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        if have != page.content {
            if rewrite {
                std::fs::create_dir_all(&dir).expect("a writable docs tree");
                std::fs::write(&path, &page.content).expect("a writable page");
                wrote += 1;
            } else {
                let (have, mine): (Vec<&str>, Vec<&str>) =
                    (have.lines().collect(), page.content.lines().collect());
                let at = (0..have.len().max(mine.len()))
                    .find(|i| have.get(*i) != mine.get(*i))
                    .unwrap_or(0);
                findings.push(format!(
                    "reference/cli/{}:{} is stale\n    have: {}\n    want: {}",
                    page.file,
                    at + 1,
                    have.get(at).copied().unwrap_or_default(),
                    mine.get(at).copied().unwrap_or_default()
                ));
            }
        }
    }

    // The other direction: a file the walk no longer produces is a page for
    // a command that no longer exists.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries {
            let name = entry.expect("a readable directory entry").file_name();
            let name = name.to_string_lossy().to_string();
            if !pages.iter().any(|page| page.file == name) {
                if rewrite {
                    std::fs::remove_file(dir.join(&name)).expect("a removable stray");
                    wrote += 1;
                } else {
                    findings.push(format!(
                        "reference/cli/{name} is not a page the generator produces"
                    ));
                }
            }
        }
    }

    if rewrite {
        println!("rewrote {wrote} files");
    }

    let mut report = String::new();
    for finding in &findings {
        report.push_str(finding);
        report.push('\n');
    }
    if !report.is_empty() {
        report.push_str("  run: make docs-gen\n");
    }
    assert!(report.is_empty(), "\n{report}");
}

/// The registry region of `docs/reference/config.md`, markers included: one
/// `###` entry per [`crate::cmd::config::Setting`] — git key, kind, default,
/// and the description joined back into the one paragraph it wraps.
fn registry_region() -> String {
    use crate::cmd::config::{SettingKind, registry};

    let mut out = String::from(
        "<!-- registry:begin — generated from registry() in \
         crates/ff-cli/src/cmd/config.rs by a test; edit there, then make docs-gen -->\n",
    );
    for setting in registry() {
        let _ = write!(out, "\n### {}\n\n", setting.name);
        match &setting.kind {
            SettingKind::Choice(valid) => {
                let list = valid
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "`{}` — choice of {list}; default `{}`",
                    setting.key, setting.def
                );
            }
            kind if setting.def.is_empty() => {
                let _ = writeln!(
                    out,
                    "`{}` — {}; unset by default",
                    setting.key,
                    kind.label()
                );
            }
            kind => {
                let _ = writeln!(
                    out,
                    "`{}` — {}; default `{}`",
                    setting.key,
                    kind.label(),
                    setting.def
                );
            }
        }
        let _ = write!(out, "\n{}\n", setting.desc.join(" "));
    }
    out.push_str("\n<!-- registry:end -->\n");
    out
}

/// The region of `page` between the markers equals `want`, byte for byte;
/// the prose around them is hand-written and never touched. `FF_DOCS_GEN=1`
/// splices the fresh region in instead of reporting the drift. `begin_prefix`
/// is the opening marker up to its first space, so the generator's note after
/// it can change without moving the region.
fn region_is_generated(page: &str, begin_prefix: &str, end_marker: &str, want: String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reference")
        .join(page);
    let have = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("a readable docs/reference/{page}"));
    let begin = have
        .find(begin_prefix)
        .unwrap_or_else(|| panic!("{page}: no `{begin_prefix}` marker"));
    let end = have
        .find(end_marker)
        .unwrap_or_else(|| panic!("{page}: no `{}` marker", end_marker.trim_end()))
        + end_marker.len();
    assert!(begin < end, "{page}: region markers out of order");

    if have[begin..end] == want {
        return;
    }

    if std::env::var_os("FF_DOCS_GEN").is_some() {
        let fresh = format!("{}{}{}", &have[..begin], want, &have[end..]);
        std::fs::write(&path, fresh).unwrap_or_else(|_| panic!("a writable {page}"));
        println!("rewrote the generated region of {page}");
        return;
    }

    let (have, mine): (Vec<&str>, Vec<&str>) =
        (have[begin..end].lines().collect(), want.lines().collect());
    let at = (0..have.len().max(mine.len()))
        .find(|i| have.get(*i) != mine.get(*i))
        .unwrap_or(0);
    panic!(
        "\nreference/{page}: the generated region is stale\n    have: {}\n    want: {}\n  \
         run: make docs-gen\n",
        have.get(at).copied().unwrap_or_default(),
        mine.get(at).copied().unwrap_or_default()
    );
}

/// The settings registry region of `docs/reference/config.md` is generated.
#[test]
fn the_config_registry_is_generated() {
    region_is_generated(
        "config.md",
        "<!-- registry:begin",
        "<!-- registry:end -->\n",
        registry_region(),
    );
}

/// The error index region of `docs/reference/errors.md`, markers included:
/// one table row per [`crate::explain::ENTRIES`] entry, sorted by id so the
/// namespaces group without headings and the region does not move when the
/// registry is reordered. The row is id, exit code, summary; the detail
/// stays with `ff explain <id>`.
fn errors_region() -> String {
    let mut out = String::from(
        "<!-- errors:begin — generated from ENTRIES in crates/ff-cli/src/explain.rs by a \
         test; edit there, then make docs-gen -->\n\n| id | exit | meaning |\n| --- | --- | --- |\n",
    );
    let mut entries: Vec<&crate::explain::Entry> = crate::explain::ENTRIES.iter().collect();
    entries.sort_by_key(|e| e.id);
    for entry in entries {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} |",
            entry.id,
            ff_core::exit_code_for(entry.id),
            entry.summary
        );
    }
    out.push_str("\n<!-- errors:end -->\n");
    out
}

/// The error index region of `docs/reference/errors.md` is generated.
#[test]
fn the_error_index_is_generated() {
    region_is_generated(
        "errors.md",
        "<!-- errors:begin",
        "<!-- errors:end -->\n",
        errors_region(),
    );
}
