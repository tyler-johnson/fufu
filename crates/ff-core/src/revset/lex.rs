//! The revset scanner. It has exactly two states — between tokens, or inside
//! a revision token — and no third.
//!
//! That is the whole design. [`scan_revision`] consumes a base and then every
//! gitrevisions suffix that follows it, so in `main~2` the cursor passes the
//! `~` while it is *inside* a revision token and the complement arm below
//! never sees that byte. In `~main` the cursor is between tokens, so it does.
//! One byte, two meanings, settled by where the cursor is rather than by
//! lookahead or backtracking — which is what lets gitrevisions keep every
//! spelling it has while set algebra is layered around it. Lookahead was the
//! obvious alternative and it loses: `main~2` and `~main` differ by what came
//! *before* the `~`, and a scanner that already knows where it is has that for
//! free.
//!
//! What the scanner delegates is gitrevisions' *revision* grammar — its
//! symbols and its suffixes, kept whole and never reinterpreted. Range
//! spellings are not part of that: `..` and `::` here are fufu's own set
//! algebra, which is why `a...b` is refused below rather than inherited.
//!
//! Whitespace is therefore irrelevant to meaning: it separates tokens and does
//! nothing else. It survives in exactly two places, both bracketed and both
//! scanned to their closer rather than to the first candidate — a brace group,
//! where `@{2 days ago}` needs it, and a quoted pattern value, where
//! `substring:"fix bug"` does.

use std::ops::Range;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// One gitrevisions revision — base and every suffix, exactly as typed.
    /// Opaque: the front end recognizes its extent and never its meaning.
    Revision(String),
    /// `::`
    DagRange,
    /// `..`
    Range,
    /// `~` reached between tokens, which is the only position where it means
    /// complement.
    Complement,
    /// `&`
    And,
    /// `|`
    Or,
    /// `(`
    Open,
    /// `)`
    Close,
    /// `,`
    Comma,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Byte range in the source. Errors quote the user's own bytes back at
    /// them rather than a reconstruction that has quietly lost their spacing.
    pub span: Range<usize>,
}

/// The pattern kinds, as the scanner sees them: a value wearing one of these
/// prefixes may quote itself, and nothing else may. [`super::parse`] reads the
/// same list back off to name the kind, so the two halves cannot drift.
pub(super) const PATTERN_PREFIXES: [&str; 4] = ["exact:", "glob:", "substring:", "regex:"];

/// Scan a revset expression into tokens. Errors here are the ones a scanner
/// can see on its own: an unclosed bracket, and the jj spellings fufu refuses
/// on purpose.
pub fn lex(src: &str) -> Result<Vec<Token>> {
    let b = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let kind = match c {
            b'&' => {
                i += 1;
                TokenKind::And
            }
            b'|' => {
                i += 1;
                TokenKind::Or
            }
            b'(' => {
                i += 1;
                TokenKind::Open
            }
            b')' => {
                i += 1;
                TokenKind::Close
            }
            b',' => {
                i += 1;
                TokenKind::Comma
            }
            b'~' => {
                i += 1;
                TokenKind::Complement
            }
            b':' if b.get(i + 1) == Some(&b':') => {
                i += 2;
                TokenKind::DagRange
            }
            b'.' if b.get(i + 1) == Some(&b'.') => {
                if b.get(i + 2) == Some(&b'.') {
                    return Err(no_symmetric_difference());
                }
                i += 2;
                TokenKind::Range
            }
            // A `^` between tokens has no base to attach to. git's rev-list
            // spells exclusion that way and the muscle memory is real, so say
            // which character fufu uses instead of reporting a stray byte.
            b'^' => return Err(caret_is_not_complement(src)),
            // These reach token position only when a complete token already
            // ended — `main~2+`, `latest(x)-`. The base-trailing check in
            // scan_revision catches the far commoner `main+` and `main-`.
            b'+' => return Err(deferred_descendants(preceding(&out))),
            b'-' => return Err(parent_shorthand(preceding(&out))),
            _ => scan_revision(src, &mut i)?,
        };
        out.push(Token {
            kind,
            span: start..i,
        });
    }
    Ok(out)
}

/// Consume one revision token: a base, then every suffix that follows it. The
/// cursor never returns to token position part way through, which is what
/// makes `main~2` one token and `~main` two.
fn scan_revision(src: &str, i: &mut usize) -> Result<TokenKind> {
    let start = *i;
    scan_base(src, start, i)?;
    refuse_shorthand(&src[start..*i])?;
    while scan_suffix(src, i)? {}
    Ok(TokenKind::Revision(src[start..*i].to_string()))
}

/// The base is everything up to a character that cannot belong to one. Note
/// what is *not* here: `-`, `+`, `/`, `{`, `}`, `"` and a lone `.` or `:` are
/// all ordinary, because ref names and gitrevisions' own `:` forms use them.
fn scan_base(src: &str, start: usize, i: &mut usize) -> Result<()> {
    let b = src.as_bytes();
    while *i < b.len() {
        match b[*i] {
            c if c.is_ascii_whitespace() => break,
            b'&' | b'|' | b'(' | b')' | b',' => break,
            // Suffix starts. Breaking here hands them to scan_suffix, which is
            // still inside this token — the cursor does not go back to token
            // position, so a `~` here can never be read as complement.
            b'^' | b'~' => break,
            b':' if b.get(*i + 1) == Some(&b':') => break,
            b'.' if b.get(*i + 1) == Some(&b'.') => break,
            b'@' if b.get(*i + 1) == Some(&b'{') => break,
            // A quote is special only where a pattern value begins, so a ref
            // that happens to contain one is untouched. Consuming the run here
            // rather than in a state of its own is what keeps a regex's `^`,
            // `~`, and `|` from being read as operators: the cursor is inside
            // a revision token the whole way through, exactly as for `^{…}`.
            b'"' if PATTERN_PREFIXES.contains(&&src[start..*i]) => scan_quoted(src, i)?,
            _ => *i += 1,
        }
    }
    // A base can be empty only at a bare `@{…}`, which gitrevisions allows.
    Ok(())
}

/// Consume one gitrevisions suffix if the cursor is on one. Returns whether it
/// consumed anything, so the caller can loop: `main^2~3^{tree}` is one token.
fn scan_suffix(src: &str, i: &mut usize) -> Result<bool> {
    let b = src.as_bytes();
    match b.get(*i) {
        Some(b'^') => {
            *i += 1;
            match b.get(*i) {
                // `^{tree}`, `^{/regex}`, `^{}`
                Some(b'{') => scan_braces(src, i)?,
                // `^!` and `^@`
                Some(b'!' | b'@') => *i += 1,
                // `^-n`
                Some(b'-') => {
                    *i += 1;
                    scan_digits(b, i);
                }
                // `^` and `^n`
                _ => scan_digits(b, i),
            }
            Ok(true)
        }
        // `~` and `~n`
        Some(b'~') => {
            *i += 1;
            scan_digits(b, i);
            Ok(true)
        }
        // `@{…}` — the reflog suffix, and the reason `@` is not plain meta.
        Some(b'@') if b.get(*i + 1) == Some(&b'{') => {
            *i += 1;
            scan_braces(src, i)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Consume a brace group by nesting depth. Neither the first `}` nor the first
/// space can end one: `@{2 days ago}` carries spaces and `^{/re{gex}}` carries
/// braces, and both are gitrevisions fufu promised to keep whole.
fn scan_braces(src: &str, i: &mut usize) -> Result<()> {
    let b = src.as_bytes();
    let open = *i;
    *i += 1;
    let mut depth = 1usize;
    while *i < b.len() {
        match b[*i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    *i += 1;
                    return Ok(());
                }
            }
            _ => {}
        }
        *i += 1;
    }
    Err(unterminated_brace(&src[open..]))
}

/// Consume a quoted pattern value. Two escapes and no more — `\"` so the value
/// can carry a quote, `\\` so it can end in a backslash — because the text
/// inside is glob and regex, whose own backslashes must reach the resolver
/// untouched. An escape zoo here would silently eat `\d`.
fn scan_quoted(src: &str, i: &mut usize) -> Result<()> {
    let b = src.as_bytes();
    let open = *i;
    *i += 1;
    while *i < b.len() {
        match b[*i] {
            b'\\' if *i + 1 < b.len() => *i += 2,
            b'"' => {
                *i += 1;
                return Ok(());
            }
            _ => *i += 1,
        }
    }
    Err(unterminated_quote(&src[open..]))
}

fn scan_digits(b: &[u8], i: &mut usize) {
    while b.get(*i).is_some_and(u8::is_ascii_digit) {
        *i += 1;
    }
}

/// `x-` and `x+` are jj's, and both are refused by rule rather than by taste —
/// one is `x^` respelled, the other names a walk fufu has no index for. A
/// trailing sign is the only shape that can mean them: `my-branch` and
/// `c++-port` carry the same characters in the middle and stay ordinary names.
fn refuse_shorthand(base: &str) -> Result<()> {
    // A one-character base is the sign itself; let it through as an opaque
    // revision so the message below always has a name to put in front of it.
    if base.len() < 2 {
        return Ok(());
    }
    // `@-` is a respelling in both address spaces at once, which is why it
    // survives in neither. Naming a commit it is `HEAD`: the open change sits
    // on HEAD's commit, so "the commit under `@`" is what git already says.
    // Naming an operation it is `@^`, because an operation's first parent is
    // the operation before it — so `@-3` is `@~3`, git's own first-parent
    // walk, spelled twice. Rule ten excludes both, and whether the token reads
    // as a symbol or as a suffix never enters into it.
    //
    // Refusing beats aliasing here because git's own `@` means HEAD. A fufu
    // where `@` is the open change and `@-` is HEAD would leave git's meaning
    // one keystroke from a different one, which is the collision most likely
    // to be learned wrong on the first try and never revisited.
    if let Some(rest) = base.strip_prefix("@-")
        && (rest.is_empty() || rest.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(open_change_parent(rest));
    }
    match base.as_bytes()[base.len() - 1] {
        b'+' => Err(deferred_descendants(Some(&base[..base.len() - 1]))),
        b'-' => Err(parent_shorthand(Some(&base[..base.len() - 1]))),
        _ => Ok(()),
    }
}

/// The last revision text emitted, for naming the `x` in a shorthand message.
/// Anything else (a `)`, an operator) leaves the message generic rather than
/// quoting a fragment that would read as noise.
fn preceding(tokens: &[Token]) -> Option<&str> {
    match tokens.last().map(|t| &t.kind) {
        Some(TokenKind::Revision(text)) => Some(text),
        _ => None,
    }
}

/// `@-` says something both address spaces already say, so the message has to
/// name the spelling for each rather than pick one and leave the other reader
/// guessing which half applied to them.
fn open_change_parent(back: &str) -> Error {
    let message = if back.is_empty() {
        "no `@-`: the commit under the open change is `HEAD`, and the operation \
         before this one is `@^`"
            .to_string()
    } else {
        format!(
            "no `@-{back}`: the operation {back} back is `@~{back}`; commits are `HEAD` \
             and git's own suffixes"
        )
    };
    Error::coded(
        "usage/revset-parent-shorthand",
        message,
        vec!["ff log -r HEAD".into(), "ff op show @^".into()],
    )
}

fn deferred_descendants(of: Option<&str>) -> Error {
    let x = of.unwrap_or("x");
    Error::coded(
        "revset/deferred-descendants",
        format!(
            "no `{x}+` yet: children are a reverse walk with no index behind them. \
             `{x}::` is the descendant set that does exist, unbounded"
        ),
        vec![format!("ff log -r \"{x}::\"")],
    )
}

fn parent_shorthand(of: Option<&str>) -> Error {
    let x = of.unwrap_or("x");
    Error::coded(
        "usage/revset-parent-shorthand",
        format!("fufu has no `{x}-`; the parent is `{x}^`"),
        vec![format!("ff log -r \"{x}^\"")],
    )
}

/// Not a gap in "gitrevisions entire": what fufu takes entire is the revision
/// grammar, while ranges are its own set algebra. In a set language `a...b`
/// *is* `(a..b) | (b..a)`, so admitting it would be a second way to say what
/// the language already says — the same rule that keeps out `x-` and infix `~`.
fn no_symmetric_difference() -> Error {
    Error::coded(
        "usage/revset-no-symmetric-difference",
        "fufu has no `a...b`: ranges are its own set algebra, and `(a..b) | (b..a)` \
         already says it",
        vec!["ff log -r \"(a..b) | (b..a)\"".into()],
    )
}

fn caret_is_not_complement(src: &str) -> Error {
    Error::coded(
        "usage/revset-expected-expression",
        format!("expected a revision before `^` in `{src}`; fufu spells complement `~x`, not `^x`"),
        vec![],
    )
}

fn unterminated_brace(from: &str) -> Error {
    Error::coded(
        "usage/revset-unterminated-brace",
        format!("unclosed `{{` in `{from}`"),
        vec![],
    )
}

fn unterminated_quote(from: &str) -> Error {
    Error::coded(
        "usage/revset-unterminated-quote",
        format!("unclosed `\"` in `{from}`"),
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src)
            .expect("lexes")
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    fn rev(text: &str) -> TokenKind {
        TokenKind::Revision(text.into())
    }

    #[test]
    fn one_byte_two_meanings() {
        // The whole reason the scanner has states.
        assert_eq!(kinds("main~2"), vec![rev("main~2")]);
        assert_eq!(kinds("~main"), vec![TokenKind::Complement, rev("main")]);
        assert_eq!(
            kinds("~main~2"),
            vec![TokenKind::Complement, rev("main~2")],
            "one complement, one revision — never three tokens"
        );
    }

    #[test]
    fn meta_terminates_a_base_with_no_spaces() {
        assert_eq!(
            kinds("a&~b"),
            vec![rev("a"), TokenKind::And, TokenKind::Complement, rev("b")]
        );
    }

    #[test]
    fn every_gitrevisions_suffix_is_one_token() {
        for src in [
            "a^",
            "a^2",
            "a^{tree}",
            "a^{/regex}",
            "a^{}",
            "a^!",
            "a^@",
            "a^-1",
            "a^-",
            "a~",
            "a~3",
            "a@{upstream}",
            "a@{2}",
            "a@{2 days ago}",
            "@{2 days ago}",
            "a^2~3^{tree}",
            "a^{/re{gex}}",
            "refs/heads/dead",
            "origin/main@{1}",
            "HEAD:README.md",
            ":/fixup",
            "@",
        ] {
            assert_eq!(kinds(src), vec![rev(src)], "{src} must scan as one token");
        }
    }

    /// `@-` names HEAD, which git already spells, so revision space refuses it
    /// by the same rule that keeps `x-` out. It stays legal in the operation
    /// space, and the message has to point there rather than just say no.
    #[test]
    fn open_change_parent_is_a_respelling_in_both_spaces() {
        let bare = lex("@-").expect_err("refused");
        assert_eq!(bare.id(), "usage/revset-parent-shorthand");
        let msg = bare.to_string();
        assert!(msg.contains("HEAD"), "must name the commit spelling: {msg}");
        assert!(
            msg.contains("`@^`"),
            "must name the operation spelling: {msg}"
        );

        // The numbered form is git's own first-parent walk, spelled twice.
        for (src, want) in [("@-3", "`@~3`"), ("@-12", "`@~12`")] {
            let err = lex(src).expect_err("refused");
            assert_eq!(err.id(), "usage/revset-parent-shorthand", "{src}");
            assert!(
                err.to_string().contains(want),
                "{src} must name {want}: {err}"
            );
        }

        // A branch that merely starts with the same bytes is untouched.
        assert_eq!(kinds("@-ish"), vec![rev("@-ish")]);
    }

    #[test]
    fn ancestry_operators_split_a_base() {
        assert_eq!(kinds("a::b"), vec![rev("a"), TokenKind::DagRange, rev("b")]);
        assert_eq!(
            kinds("v1.0..v2.0"),
            vec![rev("v1.0"), TokenKind::Range, rev("v2.0")]
        );
        assert_eq!(kinds("::a"), vec![TokenKind::DagRange, rev("a")]);
        assert_eq!(kinds("a.."), vec![rev("a"), TokenKind::Range]);
    }

    #[test]
    fn calls_and_patterns() {
        assert_eq!(
            kinds("description(glob:fix*)"),
            vec![
                rev("description"),
                TokenKind::Open,
                rev("glob:fix*"),
                TokenKind::Close
            ]
        );
        assert_eq!(
            kinds("f(a,b)"),
            vec![
                rev("f"),
                TokenKind::Open,
                rev("a"),
                TokenKind::Comma,
                rev("b"),
                TokenKind::Close
            ]
        );
    }

    #[test]
    fn spans_quote_the_source_back() {
        let tokens = lex("main & ~old").expect("lexes");
        assert_eq!(&"main & ~old"[tokens[0].span.clone()], "main");
        assert_eq!(&"main & ~old"[tokens[3].span.clone()], "old");
    }

    #[test]
    fn signs_stay_ordinary_inside_a_name() {
        assert_eq!(kinds("my-branch"), vec![rev("my-branch")]);
        assert_eq!(kinds("c++-port"), vec![rev("c++-port")]);
        assert_eq!(kinds("a+b"), vec![rev("a+b")]);
    }

    #[test]
    fn refused_shorthands_teach() {
        for src in ["main+", "main~2+", "latest(x)+"] {
            let err = lex(src).expect_err("refused");
            assert_eq!(err.id(), "revset/deferred-descendants", "{src}");
            assert!(err.to_string().contains("::"), "{src} must name `x::`");
        }
        for src in ["main-", "main~2-"] {
            let err = lex(src).expect_err("refused");
            assert_eq!(err.id(), "usage/revset-parent-shorthand", "{src}");
            assert!(err.to_string().contains('^'), "{src} must name `x^`");
        }
        assert_eq!(
            lex("a...b").expect_err("refused").id(),
            "usage/revset-no-symmetric-difference"
        );
        assert_eq!(
            lex("^main").expect_err("refused").id(),
            "usage/revset-expected-expression"
        );
    }

    #[test]
    fn a_quoted_pattern_value_is_part_of_its_token() {
        for src in [
            r#"regex:"^fix""#,
            r#"substring:"fix bug""#,
            r#"glob:"a&b|c(d),e~f^g""#,
            r#"exact:"say \"hi\"""#,
            r#"regex:"\d+""#,
            r#"regex:"a\\""#,
            r#"exact:"""#,
        ] {
            assert_eq!(kinds(src), vec![rev(src)], "{src} must scan as one token");
        }
        // The metacharacters inside stay inside: one operand, no operators.
        assert_eq!(
            kinds(r#"description(regex:"^a|b")"#),
            vec![
                rev("description"),
                TokenKind::Open,
                rev(r#"regex:"^a|b""#),
                TokenKind::Close
            ]
        );
    }

    #[test]
    fn a_quote_is_ordinary_anywhere_else() {
        assert_eq!(kinds(r#"a"b"#), vec![rev(r#"a"b"#)]);
        assert_eq!(kinds(r#"refs/heads/a"b"#), vec![rev(r#"refs/heads/a"b"#)]);
        // Not a kind prefix, so the quote opens nothing and `&` still splits.
        assert_eq!(
            kinds(r#"weird:"a&b"#),
            vec![rev(r#"weird:"a"#), TokenKind::And, rev(r#"b"#)]
        );
    }

    #[test]
    fn unclosed_quotes_are_named() {
        for src in [r#"regex:"^fix"#, r#"exact:"a\""#] {
            assert_eq!(
                lex(src).expect_err("refused").id(),
                "usage/revset-unterminated-quote",
                "{src}"
            );
        }
    }

    #[test]
    fn unclosed_braces_are_named() {
        for src in ["main@{2", "main^{tree", "main^{/re{gex}"] {
            assert_eq!(
                lex(src).expect_err("refused").id(),
                "usage/revset-unterminated-brace",
                "{src}"
            );
        }
    }

    #[test]
    fn non_ascii_names_survive() {
        assert_eq!(kinds("feature/café"), vec![rev("feature/café")]);
    }
}
