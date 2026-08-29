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
//! the same prose `ff help <verb>` renders; nothing is written twice.
//!
//! `docs/reference/config.md` gets the same treatment in miniature: the
//! settings registry in `cmd/config.rs` renders into a marked region of that
//! page — `<!-- registry:begin -->` through `<!-- registry:end -->` — under
//! the same byte-equality and `FF_DOCS_GEN=1` contract, with the prose
//! around the markers hand-written and untouched.

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

/// One command's page: title, the markdown description verbatim, the fenced
/// usage block, and the `## Examples` section verbatim.
fn page(path: &str, cmd: &clap::Command) -> Page {
    let mut content = format!("# ff {path}\n\n");
    match source(path) {
        Some(src) => {
            let seam = src
                .find(help::SEAM)
                .unwrap_or_else(|| panic!("help/{path}: no `## Examples` heading"));
            let _ = write!(
                content,
                "{}\n\n## Usage\n\n```\n{}\n```\n\n{}",
                &src[..seam],
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

    for group in help::GROUPS {
        let _ = write!(index, "\n## {}\n\n", group.heading);
        for row in group.commands {
            let cmd = root
                .find_subcommand(row.name)
                .unwrap_or_else(|| panic!("{} is grouped but not live", row.name));
            let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
            let _ = writeln!(index, "- [`ff {0}`]({0}.md) — {1}", row.name, about);
            out.push(page(row.name, cmd));
            for sub in family(cmd) {
                let path = format!("{} {}", row.name, sub.get_name());
                let about = sub.get_about().map(ToString::to_string).unwrap_or_default();
                let _ = writeln!(
                    index,
                    "    - [`ff {path}`]({}.md) — {about}",
                    path.replace(' ', "-")
                );
                out.push(page(&path, sub));
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

/// The region between the markers equals regeneration, byte for byte; the
/// prose around them is hand-written and never touched. `FF_DOCS_GEN=1`
/// splices the fresh region in instead of reporting the drift.
#[test]
fn the_config_registry_is_generated() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/config.md");
    let have = std::fs::read_to_string(&path).expect("a readable docs/reference/config.md");
    let begin = have
        .find("<!-- registry:begin")
        .expect("config.md: no `<!-- registry:begin` marker");
    let end_marker = "<!-- registry:end -->\n";
    let end = have
        .find(end_marker)
        .expect("config.md: no `<!-- registry:end -->` marker")
        + end_marker.len();
    assert!(begin < end, "config.md: registry markers out of order");

    let want = registry_region();
    if have[begin..end] == want {
        return;
    }

    if std::env::var_os("FF_DOCS_GEN").is_some() {
        let fresh = format!("{}{}{}", &have[..begin], want, &have[end..]);
        std::fs::write(&path, fresh).expect("a writable config.md");
        println!("rewrote the registry region");
        return;
    }

    let (have, mine): (Vec<&str>, Vec<&str>) =
        (have[begin..end].lines().collect(), want.lines().collect());
    let at = (0..have.len().max(mine.len()))
        .find(|i| have.get(*i) != mine.get(*i))
        .unwrap_or(0);
    panic!(
        "\nreference/config.md: the registry region is stale\n    have: {}\n    want: {}\n  \
         run: make docs-gen\n",
        have.get(at).copied().unwrap_or_default(),
        mine.get(at).copied().unwrap_or_default()
    );
}
