//! `fufu.toolPolicy` — what fufu says when an agent runs `ff` in its shell
//! while the `ff` tool is up for it.
//!
//! The briefing tells the agent to call the tool instead of the shell, and
//! prose does not get to 100%: a client that has already loaded its shell
//! tool and deferred the fufu one will run `ff status` through the shell
//! anyway. The one deterministic lever is the PreToolUse channel fufu
//! already owns for `fufu.gitPolicy`. **observe** says nothing. **coach**
//! names the tool once per session, as context. **strict**, the default,
//! refuses the call and names the tool and the exact `args` to call it
//! with. None of them runs anything in the shell command's place.
//!
//! The refusal fires only when fufu's own server is verifiably up for the
//! client making the call — `cmd::mcp::presence` decides that — so a
//! repository with the setting at its default and no server running
//! behaves exactly as before. Where anything cannot be determined, nothing
//! is said: the same fail-open doctrine as gitPolicy.
//!
//! fufu's own server is the only one asked about. A declared extension may
//! have a server of its own registered beside it, but that server is a
//! process the client starts and fufu never sees, so it holds no marker
//! and nothing here can tell a running one from a registered one. Refusing
//! on a registration would be refusing on a guess, and this refusal only
//! ever names the one tool, so what an extension's own server is up to
//! does not enter into it.
//!
//! Unlike gitPolicy, a compound command is read per segment rather than
//! failing open as a whole. gitPolicy's doctrine is *never refuse what you
//! cannot read with certainty*, and a segment whose first token is `ff` is
//! read with certainty; `cd x && ff status` is exactly the case the tool's
//! `cwd` argument exists for.
//!
//! There is no tally: the operation log already records every `ff` run,
//! from a shell or a tool, and `ff doctor` has nothing to add.
//!
//! What is refused follows the extension registry. A builtin verb and a
//! declared extension are both served by the tool, so both are refused and
//! pointed at it. An `ff <name>` nobody declared passes, because a shell is
//! the only place an undeclared extension runs — the tool does not serve
//! one — and a repository that refused it in both places would have nowhere
//! to run it at all. A declared extension whose manifest says its writes
//! are not undoable passes for that same reason: the one tool's args array
//! will not relay one. Such an extension may have produced tools of its
//! own, which this refusal deliberately says nothing about — the registry
//! records that an extension promised tools and never which verb each one
//! covers, and asking the binary is a spawn on every shell command the
//! agent runs. The registry is read only for a word that is neither
//! shell-only nor a builtin verb, so `ff status` reaches no file; the read
//! itself is `registry::read`, one cached parse and no PATH walk.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::cmd::mcp::child::EXCLUDED;

/// What to do about `ff` in the shell while the tool is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Say nothing.
    Observe,
    /// Name the tool, once per session.
    Coach,
    /// Refuse, naming the tool and the call to make.
    Strict,
}

/// Read the tier. A total match with a default arm, so an absent value, an
/// unreadable one, and a misspelled one are all `strict`.
pub fn read(repo: &ff_core::gix::Repository) -> Policy {
    let raw = repo
        .config_snapshot()
        .string("fufu.toolPolicy")
        .map(|value| value.to_string());
    match raw.as_deref() {
        Some(value) => match value.to_lowercase().as_str() {
            "observe" => Policy::Observe,
            "coach" => Policy::Coach,
            _ => Policy::Strict,
        },
        None => Policy::Strict,
    }
}

/// A shell `ff` the tool serves, and what the tool would be called with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The words after `ff`, as the tool's `args` would carry them.
    pub args: Vec<String>,
    /// The declared extension the first word names, when it names one
    /// rather than a builtin verb. What lets the refusal say which it is.
    pub extension: Option<String>,
}

/// The first segment of a shell command that invokes an `ff` the tool
/// serves — or `None` when no segment does.
///
/// Segments split on `&&`, `||`, `|`, `&`, `;`, and newlines that stand
/// outside quotes, backticks, and `$(…)`, so a string or a substitution
/// that mentions `ff` is not a call to it; reading stops at a heredoc,
/// whose body is prose. A segment wrapped in one pair of `(…)` or `{…}`
/// is read without the wrapper. A segment invokes `ff` when its first
/// whitespace token is exactly `ff`:
/// `/usr/bin/ff`, `target/debug/ff`, `sudo ff`, `FOO=x ff`, and `$(ff …)`
/// do not count, for the reason `rawgit` counts only a bare `git` — a path
/// names a binary fufu did not resolve, and in fufu's own repository
/// `target/dogfood/ff status` is a test of a build.
///
/// Two kinds of first word pass. One of [`EXCLUDED`] does, because those
/// verbs are shell-only. An extension nobody declared does too — a word
/// that is no builtin verb and is not on the registry — because a shell is
/// the only place one runs. Everything else matches: a builtin verb, a
/// declared extension, and a line whose first word is no verb at all,
/// `--help`, `-C <dir>`, and `--session` included, because the tool serves
/// all of those. A `--json` token is dropped, since the tool adds it, and a
/// bare `ff` becomes `["map"]`, which is what bare `ff` is.
///
/// Words are whitespace-split and quotes are kept verbatim in the tokens:
/// `ff commit -m "x y"` yields `["commit", "-m", "\"x", "y\""]`. The words
/// are a suggestion the agent rewrites into a call, not a command line
/// fufu composes for it.
pub fn classify(command: &str) -> Option<Call> {
    classify_in(command, crate::registry::read())
}

/// [`classify`] against a registry of the caller's own. The tests' door,
/// and the reason the one cached reader stays a detail of `classify`.
fn classify_in(command: &str, registry: &crate::registry::Registry) -> Option<Call> {
    for segment in segments(command) {
        let segment = unwrap(segment.trim());
        let mut tokens = segment.split_whitespace();
        if tokens.next() != Some("ff") {
            continue;
        }
        let words: Vec<&str> = tokens.collect();
        let extension = match served(words.first().copied(), registry) {
            Some(extension) => extension,
            None => continue,
        };
        let mut args: Vec<String> = words
            .into_iter()
            .filter(|word| *word != "--json")
            .map(str::to_string)
            .collect();
        if args.is_empty() {
            args.push("map".to_string());
        }
        return Some(Call { args, extension });
    }
    None
}

/// Whether the tool serves a segment whose first word after `ff` is this,
/// and the declared extension it names when it names one.
///
/// `Some(None)` is a builtin verb, or a line with no verb in it at all —
/// `ff --help`, `ff -C sub status`, bare `ff`. `Some(Some(name))` is a
/// declared extension. `None` is shell-only: one of [`EXCLUDED`], an
/// extension nobody declared, or a declared one whose manifest says its
/// writes are not undoable.
///
/// That last one is the relay's own refusal read back. The one tool's
/// annotations say nothing it relays is destructive, so its args array will
/// not carry an extension declaring `undoable: false`, and a shell has to
/// be left holding it for the reason it is left holding an undeclared one:
/// refused in both places, the verb would have nowhere at all to run.
///
/// The filter stays even though such an extension's produced tools are now
/// served, because what this refusal can name is only ever the one `ff`
/// tool and an args array, and that is the route the extension is refused
/// on. Naming a `<name>__<verb>` instead would be a guess: the registry
/// says an extension promised tools, never which ones, never which verb
/// each covers, and never whether the handshake that would have produced
/// them answered at all. A guess that missed would refuse the shell and
/// send the agent to a tool that is not there, which is the one outcome
/// this rule exists to prevent. Reading the truth costs a spawn per shell
/// command, and this path fails open rather than pay it.
fn served(first: Option<&str>, registry: &crate::registry::Registry) -> Option<Option<String>> {
    let Some(first) = first else {
        // Bare `ff` is the map, which the tool serves.
        return Some(None);
    };
    if EXCLUDED.contains(&first) {
        return None;
    }
    if builtin(first) {
        return Some(None);
    }
    // A word clap would decline. It names an extension when it is spelled
    // like one, and a flag or a global's value otherwise, which keeps the
    // behavior it has always had.
    if !crate::ext::valid_name(first) {
        return Some(None);
    }
    registry
        .get(first)
        .filter(|declared| declared.manifest.undoable)
        .map(|declared| Some(declared.name().to_string()))
}

/// Whether the word names a builtin verb.
///
/// The live command tree is the answer, aliases and the hidden foreign
/// verbs included, so the set cannot drift from what clap takes. The tree
/// is not built — `help` is clap's own subcommand and exists only in a
/// built one, so it is named here — and the set is collected once, because
/// the steer reads every shell command the agent runs.
///
/// The relay asks the same question of the first word of a tool call, and
/// asks it here rather than a second time of its own: the two refusals are
/// halves of one rule, and a word one of them read as a verb and the other
/// as an extension would be refused in both places or in neither.
pub(crate) fn builtin(word: &str) -> bool {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            use clap::CommandFactory;
            crate::cli::Cli::command()
                .get_subcommands()
                .flat_map(|sub| std::iter::once(sub.get_name()).chain(sub.get_all_aliases()))
                .chain(std::iter::once("help"))
                .map(str::to_string)
                .collect()
        })
        .contains(word)
}

/// The command cut at every separator that is not inside a quoted string,
/// a backtick, or a `$(…)`, and cut short at a heredoc. Escapes are
/// honored so `\"` and `\;` do not open or split. An unterminated quote
/// swallows the rest, which reads as one segment.
fn segments(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut subst = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();
        if let Some(open) = quote {
            if c == b'\\' && open != b'\'' {
                i += 2;
                continue;
            }
            if c == open {
                quote = None;
            }
            i += 1;
            continue;
        }
        let mut cut = 0;
        match c {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' | b'"' | b'`' => quote = Some(c),
            b'$' if next == Some(b'(') => {
                subst += 1;
                i += 2;
                continue;
            }
            b')' if subst > 0 => subst -= 1,
            b'<' if next == Some(b'<') => {
                out.push(&command[start..i]);
                return out;
            }
            _ if subst > 0 => {}
            b'&' if next == Some(b'&') => cut = 2,
            b'|' if next == Some(b'|') => cut = 2,
            b'|' | b';' | b'\n' => cut = 1,
            // Background, unless it is the `&` of `2>&1` or `&>`.
            b'&' if next != Some(b'>') && bytes.get(i.wrapping_sub(1)) != Some(&b'>') => cut = 1,
            _ => {}
        }
        if cut > 0 {
            out.push(&command[start..i]);
            i += cut;
            start = i;
            continue;
        }
        i += 1;
    }
    out.push(&command[start.min(command.len())..]);
    out
}

/// One matching pair of `(…)` or `{…}` around a segment, removed.
fn unwrap(segment: &str) -> &str {
    for (open, close) in [('(', ')'), ('{', '}')] {
        if let Some(inner) = segment.strip_prefix(open) {
            return inner.strip_suffix(close).unwrap_or(inner).trim();
        }
    }
    segment
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine that has declared nothing, which is the common one.
    fn bare() -> crate::registry::Registry {
        crate::registry::load(None)
    }

    /// A registry with one name on it, written the way `ff extension add`
    /// writes one. The directory is returned so the caller keeps it alive.
    fn declaring(name: &str) -> (tempfile::TempDir, crate::registry::Registry) {
        declaring_undoable(name, true, false)
    }

    /// The same, saying whether the extension's writes can be taken back
    /// and whether its manifest promises tools.
    fn declaring_undoable(
        name: &str,
        undoable: bool,
        tools: bool,
    ) -> (tempfile::TempDir, crate::registry::Registry) {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file = dir.path().join("extensions.json");
        let body = serde_json::json!({
            "ff": crate::machine::CONTRACT,
            "extensions": [{
                "path": format!("/usr/local/bin/ff-{name}"),
                "declared_at": 1788462398,
                "manifest": {
                    "name": name,
                    "version": "0.4.1",
                    "contract": crate::machine::CONTRACT,
                    "verbs": [{"name": "brief", "read_only": true}],
                    "undoable": undoable,
                    "tools": tools,
                },
            }],
        });
        std::fs::write(&file, body.to_string()).expect("write registry");
        let registry = crate::registry::load(Some(&file));
        assert!(registry.get(name).is_some(), "the fixture declares {name}");
        (dir, registry)
    }

    fn words(command: &str) -> Option<Vec<String>> {
        classify_in(command, &bare()).map(|call| call.args)
    }

    fn some(args: &[&str]) -> Option<Vec<String>> {
        Some(args.iter().map(|arg| arg.to_string()).collect())
    }

    #[test]
    fn a_bare_ff_verb_is_the_tools_args() {
        assert_eq!(words("ff status"), some(&["status"]));
        assert_eq!(words("ff"), some(&["map"]));
        assert_eq!(words("ff --help"), some(&["--help"]));
        assert_eq!(words("ff -C sub status"), some(&["-C", "sub", "status"]));
        assert_eq!(words("ff version"), some(&["version"]));
    }

    #[test]
    fn quotes_are_kept_verbatim_in_the_tokens() {
        assert_eq!(
            words(r#"ff commit -m "x y""#),
            some(&["commit", "-m", "\"x", "y\""])
        );
    }

    #[test]
    fn json_is_dropped_because_the_tool_adds_it() {
        assert_eq!(words("ff status --json"), some(&["status"]));
        assert_eq!(words("ff --json"), some(&["map"]));
    }

    #[test]
    fn the_shell_only_verbs_pass() {
        for verb in EXCLUDED {
            assert_eq!(words(&format!("ff {verb} x")), None, "ff {verb}");
        }
        assert_eq!(words("ff git push"), None);
        // A later segment that is not shell-only still matches.
        assert_eq!(words("ff git push && ff status"), some(&["status"]));
    }

    /// A declared extension is the tool's and an undeclared one is the
    /// shell's, which is the whole of what the registry decides here.
    #[test]
    fn a_declared_extension_is_the_tools_and_an_undeclared_one_is_not() {
        let (_dir, declared) = declaring("tower");

        let call = classify_in("ff tower brief 65", &declared).expect("declared");
        assert_eq!(call.args, ["tower", "brief", "65"]);
        assert_eq!(call.extension.as_deref(), Some("tower"));

        // The same word where nobody declared it, and another word where
        // somebody declared tower: a shell is the only place either runs.
        assert_eq!(classify_in("ff tower brief 65", &bare()), None);
        assert_eq!(classify_in("ff bay warm", &declared), None);

        // A builtin verb is the tool's too, and names no extension.
        let call = classify_in("ff status", &declared).expect("builtin");
        assert_eq!(call.extension, None);
    }

    /// The args array refuses a declared extension whose writes it cannot
    /// promise are undoable, so the shell is what is left holding it —
    /// whether or not the extension promised tools of its own. The registry
    /// says it promised them and not which verb each covers, so a refusal
    /// here could only guess, and a guess that missed would leave the verb
    /// nowhere to run.
    #[test]
    fn a_declared_extension_that_is_not_undoable_is_the_shells() {
        for tools in [false, true] {
            let (_dir, declared) = declaring_undoable("tower", false, tools);
            assert_eq!(
                classify_in("ff tower brief 65", &declared),
                None,
                "tools: {tools}"
            );
        }
    }

    /// Promising tools does not take the tool's refusal off an undoable
    /// extension: the args array still relays it, so the shell is still
    /// where it is refused.
    #[test]
    fn promising_tools_changes_nothing_for_an_undoable_extension() {
        let (_dir, declared) = declaring_undoable("tower", true, true);
        let call = classify_in("ff tower brief 65", &declared).expect("declared and undoable");
        assert_eq!(call.extension.as_deref(), Some("tower"));
    }

    /// An undeclared extension passing is one segment passing, not the
    /// command: a later segment the tool serves is still named.
    #[test]
    fn a_segment_an_undeclared_extension_owns_does_not_cover_the_next() {
        assert_eq!(words("ff tower brief && ff status"), some(&["status"]));
    }

    /// A builtin verb wins over the registry the way it wins over PATH, so
    /// a declaration under a verb's name changes nothing.
    #[test]
    fn a_builtin_verb_is_never_an_extension() {
        let (_dir, declared) = declaring("status");
        let call = classify_in("ff status", &declared).expect("builtin");
        assert_eq!(call.extension, None);
    }

    /// The builtin set is the live command tree, so an alias, a hidden
    /// foreign verb, and clap's own `help` are all builtins.
    #[test]
    fn an_alias_a_foreign_verb_and_help_are_builtins() {
        assert_eq!(words("ff st"), some(&["st"]));
        assert_eq!(words("ff ci -m x"), some(&["ci", "-m", "x"]));
        assert_eq!(words("ff bookmark"), some(&["bookmark"]));
        assert_eq!(words("ff push"), some(&["push"]));
        assert_eq!(words("ff help status"), some(&["help", "status"]));
    }

    #[test]
    fn anything_dressed_as_ff_is_not_ff() {
        assert_eq!(words("/usr/bin/ff status"), None);
        assert_eq!(words("target/dogfood/ff status"), None);
        assert_eq!(words("sudo ff status"), None);
        assert_eq!(words("FF_DEBUG=1 ff status"), None);
        assert_eq!(words("command ff status"), None);
        assert_eq!(words("echo ff"), None);
        assert_eq!(words("echo $(ff status)"), None);
        assert_eq!(words(""), None);
    }

    #[test]
    fn a_compound_command_is_read_per_segment() {
        assert_eq!(words("cd sub && ff status"), some(&["status"]));
        assert_eq!(words("ff status; git log"), some(&["status"]));
        assert_eq!(words("make || ff undo"), some(&["undo"]));
        assert_eq!(words("(ff status)"), some(&["status"]));
        assert_eq!(words("{ ff status; }"), some(&["status"]));
        assert_eq!(words("git status | ff diff"), some(&["diff"]));
        assert_eq!(words("git status\nff log -n 3"), some(&["log", "-n", "3"]));
        // The first matching segment is the one named.
        assert_eq!(words("ff status && ff log"), some(&["status"]));
        assert_eq!(words("ff status & ff log"), some(&["status"]));
        assert_eq!(words("ff status 2>&1"), some(&["status", "2>&1"]));
        assert_eq!(words("make &> out.txt; ff status"), some(&["status"]));
    }

    #[test]
    fn a_mention_inside_a_string_is_not_a_call() {
        assert_eq!(words(r#"echo "a && ff status""#), None);
        assert_eq!(words("echo 'ff status; x'"), None);
        assert_eq!(words("echo `a; ff status`"), None);
        assert_eq!(words("echo $(a && ff status)"), None);
        assert_eq!(
            words("git commit -m \"see ff status\" && ff log"),
            some(&["log"])
        );
        assert_eq!(words(r#"echo \"x\" ; ff status"#), some(&["status"]));
        assert_eq!(words(r#"echo \; ff status"#), None);
    }

    #[test]
    fn a_heredoc_body_is_prose() {
        assert_eq!(words("cat <<'EOF'\nff status\nEOF"), None);
        assert_eq!(words("cat > f <<EOF\nrun ff status && ff log\nEOF"), None);
        // Before the heredoc is still read.
        assert_eq!(
            words("ff commit -m x <<EOF\nff status\nEOF"),
            some(&["commit", "-m", "x"])
        );
    }

    #[test]
    fn an_unterminated_quote_swallows_the_rest() {
        assert_eq!(words("echo \"ff status"), None);
        assert_eq!(words("ff status \""), some(&["status", "\""]));
    }
}
