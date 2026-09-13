//! Curated error ids with prose. The single source of truth for what each
//! id means and how to leave it lives beside this file as
//! `explain/errors.toml`, parsed once on first use: `ff explain`, a failure's
//! `try:` block, and the docs generator read it, and the success path never
//! does.

use std::io::Write;
use std::sync::LazyLock;

use ff_core::{Error, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Entry {
    pub id: String,
    /// One line: what this error means.
    pub summary: String,
    /// A short paragraph: why it happens and what the exits do.
    pub detail: String,
    pub exits: Vec<String>,
}

#[derive(Deserialize)]
struct Registry {
    entry: Vec<Entry>,
}

/// The registry, in the file's order. A file that does not parse is a
/// build defect, caught by `every_registry_entry_has_prose` before it
/// reaches a terminal, so the panic names the file and nothing more.
pub static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    toml::from_str::<Registry>(include_str!("explain/errors.toml"))
        .expect("explain/errors.toml parses")
        .entry
});

/// Find an entry by id, or None.
pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

/// The `try:` block a failure prints: what the raise site said, or what the
/// id means when the site said nothing.
///
/// Most `Error::coded` calls pass `vec![]`, and that was never a claim that
/// there is no way out — the way out is a property of the id, and the id
/// already has one written down here. `ff explain branch/not-found` has
/// always said `ff branch`; the failure itself said nothing at all, so
/// an agent that hit `no branch named x` went to git rather than to the
/// verb sitting one line away. Both surfaces now read the same registry, and
/// a raise site only carries exits of its own when it knows something the id
/// does not — which is why the narrower list wins when there is one.
///
/// The last resort is the registry itself. A coded failure with nothing to
/// suggest is still a failure with prose behind it, and naming the lookup is
/// better than a dead end.
pub fn exits_for(err: &Error) -> Vec<String> {
    if !err.exits().is_empty() {
        return err.exits().to_vec();
    }
    let id = err.id();
    match find(id) {
        // `internal` is the id every uncoded error reports, and its own prose
        // says the message is the whole of what is known. Sending someone to
        // read that is the one lookup worth refusing.
        Some(entry) if entry.exits.is_empty() && id != "internal" => {
            vec![format!("ff explain {id}")]
        }
        Some(entry) => entry.exits.clone(),
        None => Vec::new(),
    }
}

/// Render one entry to stdout. When exits are present, the try: block follows.
pub fn render(entry: &Entry) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "{}", entry.id)?;
    writeln!(out, "{}", entry.summary)?;
    writeln!(out)?;
    wrap(&mut out, &entry.detail, 80)?;
    if !entry.exits.is_empty() {
        writeln!(out)?;
        writeln!(out, "  try:")?;
        for hint in &entry.exits {
            writeln!(out, "    {hint}")?;
        }
    }
    Ok(())
}

/// Render every entry as `id  exit  summary` (list mode).
pub fn render_list() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    // Compute the widest id column so the codes and summaries align.
    let max_id = ENTRIES.iter().map(|e| e.id.len()).max().unwrap_or(0);
    for entry in ENTRIES.iter() {
        writeln!(
            out,
            "{:<width$}  {}  {}",
            entry.id,
            ff_core::exit_code_for(&entry.id),
            entry.summary,
            width = max_id
        )?;
    }
    Ok(())
}

/// One entry as JSON: the registry's four fields plus the exit code the id
/// carries, so a script reads the code without knowing the namespace rule.
fn entry_json(entry: &Entry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "summary": entry.summary,
        "detail": entry.detail,
        "exits": entry.exits,
        "exit": ff_core::exit_code_for(&entry.id),
    })
}

/// Emit JSON for one entry.
pub fn emit_json(entry: &Entry) -> Result<()> {
    crate::machine::emit("explain", &entry_json(entry))
}

/// Emit JSON for the list: array of entry objects.
pub fn emit_json_list() -> Result<()> {
    let entries: Vec<serde_json::Value> = ENTRIES.iter().map(entry_json).collect();
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
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn every_registry_entry_has_prose() {
        let mut seen: Vec<&str> = Vec::new();
        for entry in ENTRIES.iter() {
            assert!(
                !seen.contains(&entry.id.as_str()),
                "duplicate id: {}",
                entry.id
            );
            seen.push(&entry.id);
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
            for id in raised_ids(&production_source(&text)) {
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

    /// Ids the registry carries that no `Error::coded` call raises.
    ///
    /// Each one is an id fufu cannot produce and must therefore explain for a
    /// different reason, stated here so a genuinely dead entry cannot hide
    /// behind a habit of adding names to this list.
    const UNRAISED: &[(&str, &str)] = &[
        (
            "repo/not-found",
            "raised structurally by Error::id() for the Discover variant, never by a coded call",
        ),
        (
            "internal",
            "the fallback id every uncoded error reports; there is nothing to raise",
        ),
    ];

    /// The mirror of the guard above, and the reason both ship.
    ///
    /// `every_raised_id_is_in_the_registry` catches an id added without prose.
    /// It cannot catch the opposite — prose left behind by an id that was
    /// removed — because removing a raise site only makes that test's job
    /// easier. `usage/needs-session` outlived `ff session` by exactly that
    /// gap: an entry a user could still reach through `ff explain --list`,
    /// describing two verbs that no longer existed.
    #[test]
    fn every_registry_entry_is_reachable() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut raised: Vec<String> = Vec::new();
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            raised.extend(raised_ids(&production_source(&text)));
        }
        assert!(
            raised.len() > 20,
            "only {} coded ids found — the source walk is broken, not the registry",
            raised.len()
        );

        let orphans: Vec<&str> = ENTRIES
            .iter()
            .map(|e| e.id.as_str())
            .filter(|id| !raised.iter().any(|r| r == id))
            .filter(|id| !UNRAISED.iter().any(|(allowed, _)| allowed == id))
            .collect();
        assert!(
            orphans.is_empty(),
            "registry entries nothing raises — delete the prose or keep the raise site: \
             {orphans:#?}"
        );
    }

    /// A file with its inline test module cut off.
    ///
    /// Test modules are allowed placeholder ids: they exercise the namespace
    /// rule, not the registry. What marks one is `#[cfg(test)] mod tests`
    /// specifically, and not the bare attribute — `revset/mod.rs` declares
    /// `#[cfg(test)] mod prop;` a third of the way down, so cutting at the
    /// first attribute silently hid the rest of that file from both guards.
    /// The forward guard could not notice, since missing a raise site only
    /// makes its job easier; the reverse one found it on the first run.
    fn production_source(text: &str) -> String {
        let mut out = text;
        for (idx, _) in text.match_indices("#[cfg(test)]") {
            let rest = text[idx + "#[cfg(test)]".len()..].trim_start();
            if rest.starts_with("mod tests") {
                out = &text[..idx];
                break;
            }
        }
        out.to_string()
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

    /// Every exit string this workspace hands a user, from both places one
    /// can come from: the registry entry, and the raise site that overrode it.
    ///
    /// The two are checked together on purpose. They are the same promise
    /// written twice — "type this next" — and a verb renamed out from under
    /// either half fails a user identically, so neither half gets to rot
    /// while the other stays honest.
    #[test]
    fn every_exit_names_live_surface() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut checked = 0usize;
        for entry in ENTRIES.iter() {
            for exit in &entry.exits {
                check_exit(exit, &format!("registry entry {}", entry.id));
                checked += 1;
            }
        }
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            for (id, exit) in raised_exits(&production_source(&text)) {
                check_exit(&exit, &format!("{id} raised in {}", file.display()));
                checked += 1;
            }
        }
        // Same reason the id walks prove they read the tree: a scanner that
        // quietly matched nothing would pass while checking nothing.
        assert!(
            checked > 100,
            "only {checked} exits found — the walk is broken, not the exits"
        );
    }

    /// One exit string, held to what the CLI actually declares.
    ///
    /// Hidden is disqualifying, not just unknown: retired spellings stay
    /// declared and hidden so typing one reaches an answer, and an exit is
    /// the one place that answer must never be where we send someone.
    fn check_exit(exit: &str, whose: &str) {
        let tokens = argv(exit);
        let Some(first) = tokens.first() else {
            panic!("{whose}: an empty exit");
        };
        // git is the other tool an exit may legitimately name — `git rebase
        // --abort` has no fufu spelling. Its surface is not ours to check.
        if first == "git" {
            return;
        }
        assert_eq!(first, "ff", "{whose}: `{exit}` names neither ff nor git");

        let root = crate::cli::Cli::command();
        let mut cmd = &root;
        let mut rest = &tokens[1..];
        while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
            assert!(
                !sub.is_hide_set(),
                "{whose}: `{exit}` sends someone to {:?}, which is hidden",
                sub.get_name()
            );
            cmd = sub;
            rest = &rest[1..];
            // Passthrough: everything after `ff git` is git's to parse.
            if cmd.get_name() == "git" {
                return;
            }
        }
        // A placeholder standing where a verb goes — `ff <verb> --at-op <op>`,
        // the shape of a flag several verbs declare. There is no one command
        // to hold it to, so the flag is checked against all of them and the
        // line is not parsed: it was never meant to be typed as written.
        let shape = rest.first().is_some_and(|tok| tok == PLACEHOLDER);
        for flag in rest.iter().filter(|tok| tok.starts_with('-')) {
            let arg = find_arg(cmd, flag)
                .or_else(|| find_arg(&root, flag))
                .or_else(|| shape.then(|| anywhere(&root, flag)).flatten())
                .unwrap_or_else(|| panic!("{whose}: `{exit}` passes {flag}, which does not exist"));
            assert!(
                !arg.is_hide_set(),
                "{whose}: `{exit}` passes {flag}, which is hidden"
            );
        }
        if shape {
            return;
        }
        if let Err(err) = <crate::cli::Cli as clap::Parser>::try_parse_from(&tokens) {
            // Not every non-Ok is a failure: clap reports `-v` and `--help` as
            // errors carrying the text they printed, which is exactly what
            // those exits are for. The grammar is what is under test.
            use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};
            assert!(
                matches!(err.kind(), DisplayVersion | DisplayHelp),
                "{whose}: `{exit}` does not parse:\n{err}"
            );
        }
    }

    /// What a placeholder becomes once `argv` has filled it.
    const PLACEHOLDER: &str = "x";

    /// The first declaration of `flag` anywhere in the tree, at any depth.
    fn anywhere<'a>(cmd: &'a clap::Command, flag: &str) -> Option<&'a clap::Arg> {
        find_arg(cmd, flag).or_else(|| cmd.get_subcommands().find_map(|sub| anywhere(sub, flag)))
    }

    /// An exit string as argv. Shell quoting is resolved, since a revset is
    /// one argument however many spaces it has, and a placeholder becomes a
    /// value — what is under test is the grammar around it.
    fn argv(exit: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut token = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for ch in exit.chars() {
            match quote {
                Some(q) if ch == q => quote = None,
                Some(_) => token.push(ch),
                None if ch == '\'' || ch == '"' => {
                    quote = Some(ch);
                    started = true;
                }
                None if ch.is_whitespace() => {
                    if started {
                        out.push(std::mem::take(&mut token));
                        started = false;
                    }
                }
                None => {
                    token.push(ch);
                    started = true;
                }
            }
        }
        if started {
            out.push(token);
        }
        out.into_iter()
            .map(|tok| {
                if tok.starts_with('<') || tok.starts_with('{') {
                    "x".to_string()
                } else {
                    tok
                }
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

    /// The exits each `Error::coded(` call passes, paired with its id.
    ///
    /// The call's last `vec![` is the exits argument; every string literal
    /// inside it is one exit, `format!` template and all — a placeholder the
    /// caller fills is still the grammar the user is shown.
    fn raised_exits(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for (idx, _) in text.match_indices("Error::coded(") {
            let body = &text[idx + "Error::coded(".len()..];
            let Some(end) = call_end(body) else { continue };
            let body = &body[..end];
            let Some(id) = literals(body).into_iter().next() else {
                continue;
            };
            let Some(vec_at) = body.rfind("vec![") else {
                continue;
            };
            for element in elements(&body[vec_at + "vec![".len()..]) {
                // The element's *first* literal: the template of a `format!`,
                // or the whole of a plain `"…".into()`. A literal further in
                // is an argument being interpolated, not an exit.
                if let Some(exit) = literals(&element).into_iter().next() {
                    found.push((id.clone(), exit));
                }
            }
        }
        found
    }

    /// A `vec![…]` body split into its elements, on commas that sit outside
    /// every bracket and every string.
    fn elements(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut depth = 0i32;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' => {
                    current.push(ch);
                    while let Some(c) = chars.next() {
                        current.push(c);
                        if c == '\\' {
                            if let Some(escaped) = chars.next() {
                                current.push(escaped);
                            }
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' | '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ']' if depth == 0 => break,
                ']' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut current)),
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
        out
    }

    /// Offset of the `)` closing a call whose `(` was just consumed, with
    /// string literals skipped so a paren inside prose does not close it.
    fn call_end(text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut depth = 1usize;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Every string literal in `text`, escapes resolved to the character.
    fn literals(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '"' {
                i += 1;
                continue;
            }
            i += 1;
            let mut lit = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                lit.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push(lit);
        }
        out
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
