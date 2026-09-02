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
//! The refusal fires only when a fufu server is verifiably up for the
//! client making the call — `cmd::mcp::presence` decides that — so a
//! repository with the setting at its default and no server running
//! behaves exactly as before. Where anything cannot be determined, nothing
//! is said: the same fail-open doctrine as gitPolicy.
//!
//! Unlike gitPolicy, a compound command is read per segment rather than
//! failing open as a whole. gitPolicy's doctrine is *never refuse what you
//! cannot read with certainty*, and a segment whose first token is `ff` is
//! read with certainty; `cd x && ff status` is exactly the case the tool's
//! `cwd` argument exists for.
//!
//! There is no tally: the operation log already records every `ff` run,
//! from a shell or a tool, and `ff doctor` has nothing to add.

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

/// The words after `ff` for the first segment of a shell command that
/// invokes it, as the tool's `args` would carry them — or `None` when no
/// segment does.
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
/// The six verbs the tool does not offer pass: a segment whose first word
/// after `ff` is one of [`EXCLUDED`] is not a match. Everything else is,
/// `--help`, `-C <dir>`, `--session`, and `version` included, because the
/// tool serves all of them. A `--json` token is dropped, since the tool
/// adds it, and a bare `ff` becomes `["map"]`, which is what bare `ff` is.
///
/// Words are whitespace-split and quotes are kept verbatim in the tokens:
/// `ff commit -m "x y"` yields `["commit", "-m", "\"x", "y\""]`. The words
/// are a suggestion the agent rewrites into a call, not a command line
/// fufu composes for it.
pub fn classify(command: &str) -> Option<Vec<String>> {
    for segment in segments(command) {
        let segment = unwrap(segment.trim());
        let mut tokens = segment.split_whitespace();
        if tokens.next() != Some("ff") {
            continue;
        }
        let words: Vec<&str> = tokens.collect();
        if words.first().is_some_and(|verb| EXCLUDED.contains(verb)) {
            continue;
        }
        let mut args: Vec<String> = words
            .into_iter()
            .filter(|word| *word != "--json")
            .map(str::to_string)
            .collect();
        if args.is_empty() {
            args.push("map".to_string());
        }
        return Some(args);
    }
    None
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

    fn words(command: &str) -> Option<Vec<String>> {
        classify(command)
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
