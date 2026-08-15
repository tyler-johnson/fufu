//! Text matching for the predicate functions — the half of a revset that asks
//! a question about a commit rather than about the graph.
//!
//! Three kinds match here and a fourth is refused. `regex:` parses in the
//! front end because a grammar that did not recognize it would report a
//! mystery, and it is refused here because the only regex engine worth
//! shipping costs about 1.5 MB of dependency and no caller has asked for one
//! yet. Recognized-and-refused names the two spellings that do work; an
//! unrecognized prefix would have named nothing.

use super::parse::PatternKind;
use crate::error::{Error, Result};

/// A compiled pattern. Compilation is where a refusal happens, so a pattern
/// that exists can always answer — matching never fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    kind: PatternKind,
    value: String,
}

impl Pattern {
    /// Compile one pattern, refusing `regex:`.
    pub fn new(kind: PatternKind, value: impl Into<String>) -> Result<Self> {
        if kind == PatternKind::Regex {
            return Err(regex_unavailable());
        }
        Ok(Pattern {
            kind,
            value: value.into(),
        })
    }

    /// Whether `text` satisfies this pattern.
    pub fn matches(&self, text: &str) -> bool {
        match self.kind {
            PatternKind::Exact => text == self.value,
            PatternKind::Substring => text.contains(&self.value),
            // git's own matcher, so `glob:release/*` means here what it means
            // in a refspec. Slashes are ordinary: a commit message is not a
            // path, and `*` that stopped at `/` would surprise.
            PatternKind::Glob => gix::glob::wildmatch(
                self.value.as_str().into(),
                text.into(),
                gix::glob::wildmatch::Mode::empty(),
            ),
            // Unreachable: `new` refuses it. Answering `false` beats a panic
            // if a future constructor ever forgets.
            PatternKind::Regex => false,
        }
    }
}

fn regex_unavailable() -> Error {
    Error::coded(
        "revset/regex-unavailable",
        "no `regex:` patterns yet: fufu ships no regex engine. `glob:` matches with \
         `*` and `?`, and `substring:` matches plain text",
        vec![
            "ff log -r \"description(glob:fix*)\"".into(),
            "ff log -r \"description(substring:fix)\"".into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(kind: PatternKind, value: &str) -> Pattern {
        Pattern::new(kind, value).expect("compiles")
    }

    #[test]
    fn exact_is_the_whole_string() {
        let pat = p(PatternKind::Exact, "fix");
        assert!(pat.matches("fix"));
        assert!(!pat.matches("fix the thing"));
        assert!(!pat.matches("prefix"));
    }

    #[test]
    fn substring_is_anywhere() {
        let pat = p(PatternKind::Substring, "fix");
        assert!(pat.matches("fix"));
        assert!(pat.matches("a prefix here"));
        assert!(!pat.matches("FIX"), "matching is case sensitive");
        assert!(
            p(PatternKind::Substring, "").matches("anything"),
            "the empty substring is in every string"
        );
    }

    #[test]
    fn glob_is_gits_own_matcher() {
        assert!(p(PatternKind::Glob, "fix*").matches("fix the thing"));
        assert!(!p(PatternKind::Glob, "fix*").matches("prefix"));
        assert!(p(PatternKind::Glob, "*fix*").matches("prefix"));
        assert!(p(PatternKind::Glob, "f?x").matches("fix"));
        // A slash is an ordinary character: a message is not a path.
        assert!(p(PatternKind::Glob, "release/*").matches("release/1.2"));
        assert!(p(PatternKind::Glob, "a*c").matches("a/b/c"));
    }

    #[test]
    fn a_metacharacter_can_be_escaped() {
        assert!(p(PatternKind::Glob, r"a\*c").matches("a*c"));
        assert!(!p(PatternKind::Glob, r"a\*c").matches("abc"));
    }

    #[test]
    fn regex_is_recognized_and_refused() {
        let err = Pattern::new(PatternKind::Regex, "^fix").expect_err("refused");
        assert_eq!(err.id(), "revset/regex-unavailable");
        assert_eq!(
            err.exit_code(),
            1,
            "a capability that is not here yet, not a command line that was wrong"
        );
        let msg = err.to_string();
        assert!(msg.contains("glob:"), "must name the alternatives: {msg}");
        assert!(msg.contains("substring:"), "{msg}");
    }
}
