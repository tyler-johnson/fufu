//! Curated error ids with prose. The single source of truth for what each
//! id means and how to leave it.

use ff_core::{Error, Result};
use std::io::Write;

pub struct Entry {
    pub id: &'static str,
    /// One line: what this error means.
    pub summary: &'static str,
    /// A short paragraph: why it happens and what the exits do.
    pub detail: &'static str,
    pub exits: &'static [&'static str],
}

pub static ENTRIES: &[Entry] = &[
    Entry {
        id: "branch/exists",
        summary: "a branch of that name already exists",
        detail: "fufu never reuses a branch name implicitly. A name that is already taken could be \
                 someone's work, and quietly landing on top of it is the one guess worth refusing. \
                 Pick another name, or switch to that branch if it was the one you meant.",
        exits: &["ff branch <name>"],
    },
    Entry {
        id: "branch/not-found",
        summary: "no branch here goes by that name",
        detail: "Names resolve against local branches, so a branch that exists on the remote but \
                 not here will not be found. Adding @<remote> fetches it and lands you on a copy. \
                 Bare ff branch lists what is local.",
        exits: &["ff branch", "ff switch <branch>@origin"],
    },
    Entry {
        id: "repo/bare",
        summary: "this is a bare repository, and the verb needs a working tree",
        detail: "A bare repository has no working tree, so there is nothing to snapshot, commit, \
                 restore into, or switch. Read-only verbs still work here; anything that touches \
                 files does not. Run the command from a clone that has a working tree.",
        exits: &[],
    },
    Entry {
        id: "repo/detached",
        summary: "HEAD is not on a branch",
        detail: "fufu keeps HEAD attached — every head is a real branch ref from the moment it \
                 exists — and the verbs that describe or record work need to know which branch \
                 the work belongs to. A detached HEAD usually means a raw git checkout of a \
                 commit; switching to a branch reattaches it.",
        exits: &["ff switch <branch>"],
    },
    Entry {
        id: "identity/missing",
        summary: "git has no name and email to sign work with",
        detail: "Every commit fufu writes carries an author, including the snapshots the capture \
                 floor takes on your behalf, so there is nothing sensible to do without an \
                 identity. Set it once globally, or per repository when this work belongs to a \
                 different one.",
        exits: &[
            "git config user.name <name>",
            "git config user.email <email>",
        ],
    },
    Entry {
        id: "restore/nothing-selected",
        summary: "restore was given nothing to restore",
        detail: "Restore is deliberately explicit: it takes the paths you name, or --all for the \
                 whole tree, and never guesses a selection on your behalf. Either form pairs with \
                 --at to choose a point in the timeline.",
        exits: &["ff restore --all", "ff restore <path> --at <id>"],
    },
    Entry {
        id: "undo/nothing",
        summary: "the journal has nothing left to undo",
        detail: "Undo walks fufu's operation journal. Either nothing has been recorded yet, or \
                 everything recorded has already been rolled back or trimmed past the keep window. \
                 ff log --ops shows what the journal still holds.",
        exits: &["ff log --ops"],
    },
    Entry {
        id: "ref/contended",
        summary: "another process is holding that ref",
        detail: "Two things tried to move the same ref at once — often a second fufu command, an \
                 editor's git integration, or a hook. Nothing was changed. Contention is a fact \
                 rather than a fault: run it again once the other write finishes.",
        exits: &[],
    },
    Entry {
        id: "hook/declined",
        summary: "one of your git hooks refused the commit",
        detail: "fufu runs your pre-commit and commit-msg hooks itself, so a hook that exits \
                 non-zero stops the close exactly as it would under git. The hook's own output \
                 says why. --no-verify skips them, with the usual caveat that they were there \
                 for a reason.",
        exits: &["ff commit --no-verify"],
    },
    Entry {
        id: "editor/failed",
        summary: "the editor did not produce a description",
        detail: "The bare form of describe seeds a temporary file and opens $EDITOR on it. When \
                 the editor cannot be launched, or exits non-zero, the description is left exactly \
                 as it was rather than half-written. Passing -m skips the editor entirely.",
        exits: &["ff describe -m <msg>"],
    },
    Entry {
        id: "target/unresolvable",
        summary: "that target is neither a branch nor a revision",
        detail: "Targets resolve through one grammar — branch names, commit shas, snapshot ids, \
                 @ and @-, trunk, and git's own suffixes — and nothing in it matched. ff log \
                 prints ids in exactly the form the resolver accepts, which is the fastest way \
                 to check a spelling.",
        exits: &["ff log", "ff evolog"],
    },
    Entry {
        id: "usage/unknown-key",
        summary: "no fufu setting goes by that name",
        detail: "Settings live in a typed registry, so a name that is not in it would silently do \
                 nothing if it were written. Bare ff config lists every setting with its value, \
                 its meaning, and whether it is still at its default.",
        exits: &["ff config"],
    },
    Entry {
        id: "usage/bad-value",
        summary: "the value did not parse as this setting's type",
        detail: "Every setting is validated through the same parser its readers use, before \
                 anything touches disk — a value that would be ignored at read time is refused at \
                 write time instead. ff config <key> shows the current value and the shape expected.",
        exits: &[],
    },
    Entry {
        id: "usage/bad-flags",
        summary: "those flags do not go together",
        detail: "The message names the combination. Flags that would contradict each other are \
                 refused rather than quietly resolved by precedence, so the command you get is \
                 always the command you wrote.",
        exits: &[],
    },
    Entry {
        id: "usage/needs-message",
        summary: "a description was needed and there was no terminal to ask on",
        detail: "The bare form of describe opens an editor, which needs a terminal; in a script, \
                 a hook, or anything running non-interactively there is nobody to answer it. Pass \
                 the text with -m instead. FF_NONINTERACTIVE forces this behavior even on a \
                 terminal.",
        exits: &["ff describe -m <msg>"],
    },
    Entry {
        id: "usage/unknown-error-id",
        summary: "no error goes by that id",
        detail: "Error ids are stable and namespaced — usage/ for a command line that was wrong, \
                 held/ for work that stopped waiting on a decision, and a bare name for \
                 everything else. ff explain --list prints every id fufu can raise.",
        exits: &["ff explain --list"],
    },
    Entry {
        id: "repo/not-found",
        summary: "no git repository here, or in any parent directory",
        detail: "fufu works inside a git repository, and searches upward from the current \
                 directory to find one. Either this is not a working tree, or you are outside \
                 the one you meant to be in.",
        exits: &[],
    },
    Entry {
        id: "internal",
        summary: "an unclassified failure",
        detail: "This error has no curated id yet: it is a failure passed through from git, the \
                 filesystem, or fufu's own internals rather than a decision waiting on you. The \
                 message is the whole of what is known. If it reproduces, it is worth reporting.",
        exits: &[],
    },
];

/// Find an entry by id, or None.
pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

/// Render one entry to stdout. When exits are present, the try: block follows.
pub fn render(entry: &Entry) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "{}", entry.id)?;
    writeln!(out, "{}", entry.summary)?;
    writeln!(out)?;
    wrap(&mut out, entry.detail, 80)?;
    if !entry.exits.is_empty() {
        writeln!(out)?;
        writeln!(out, "  try:")?;
        for hint in entry.exits {
            writeln!(out, "    {hint}")?;
        }
    }
    Ok(())
}

/// Render every entry as `id  summary` (list mode).
pub fn render_list() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    // Compute the widest id column so summaries align.
    let max_id = ENTRIES.iter().map(|e| e.id.len()).max().unwrap_or(0);
    for entry in ENTRIES {
        writeln!(
            out,
            "{:<width$}  {}",
            entry.id,
            entry.summary,
            width = max_id
        )?;
    }
    Ok(())
}

/// Emit JSON for one entry.
pub fn emit_json(entry: &Entry) -> Result<()> {
    let data = serde_json::json!({
        "id": entry.id,
        "summary": entry.summary,
        "detail": entry.detail,
        "exits": entry.exits,
    });
    crate::machine::emit("explain", &data)
}

/// Emit JSON for the list: array of entry objects.
pub fn emit_json_list() -> Result<()> {
    let entries: Vec<serde_json::Value> = ENTRIES
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "summary": e.summary,
                "detail": e.detail,
                "exits": e.exits,
            })
        })
        .collect();
    let data = serde_json::json!({ "entries": entries });
    crate::machine::emit("explain", &data)
}

/// Error when an id is not found in the registry.
pub fn unknown_id(id: &str) -> Error {
    Error::coded(
        "usage/unknown-error-id",
        format!("no such error id: {id}"),
        vec!["ff explain --list".into()],
    )
}

/// Wrap `text` to `width` columns, writing to `out`. Simple word-wrap: break
/// at spaces, never mid-word.
fn wrap(out: &mut impl Write, text: &str, width: usize) -> std::io::Result<()> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut col = 0;
    for word in words {
        if col > 0 && col + 1 + word.len() > width {
            writeln!(out)?;
            col = 0;
        }
        if col > 0 {
            write!(out, " ")?;
            col += 1;
        }
        write!(out, "{word}")?;
        col += word.len();
    }
    if col > 0 {
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_entry_has_prose() {
        let mut seen = Vec::new();
        for entry in ENTRIES {
            assert!(!seen.contains(&entry.id), "duplicate id: {}", entry.id);
            seen.push(entry.id);
            assert!(!entry.summary.is_empty(), "{}: summary is empty", entry.id);
            assert!(!entry.detail.is_empty(), "{}: detail is empty", entry.id);
        }
    }

    /// The registry is a promise, and a promise nothing checks is a promise
    /// that rots. Every id raised anywhere in the workspace must be
    /// explainable, so adding a coded error without an entry fails here
    /// rather than at a user's terminal.
    #[test]
    fn every_raised_id_is_in_the_registry() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut missing: Vec<(String, String)> = Vec::new();
        let mut found = 0usize;
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            // Test modules are allowed placeholder ids: they exercise the
            // namespace rule, not the registry. The convention here is that
            // `#[cfg(test)]` is the last thing in a file.
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            for id in raised_ids(production) {
                found += 1;
                if !ENTRIES.iter().any(|e| e.id == id) {
                    missing.push((id, file.display().to_string()));
                }
            }
        }
        // A walker that silently found nothing would pass this test while
        // checking nothing at all, so it has to prove it read the tree.
        assert!(
            found > 20,
            "only {found} coded ids found — the source walk is broken, not the registry"
        );
        assert!(
            missing.is_empty(),
            "Error::coded ids with no registry entry: {missing:#?}"
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn rust_sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
        found
    }

    /// The first string literal after each `Error::coded(` — which is the id,
    /// whether the call sits on one line or is wrapped across several.
    fn raised_ids(text: &str) -> Vec<String> {
        let mut ids = Vec::new();
        for (idx, _) in text.match_indices("Error::coded(") {
            let rest = &text[idx..];
            let Some(open) = rest.find('"') else { continue };
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else {
                continue;
            };
            ids.push(after[..close].to_string());
        }
        ids
    }
}
