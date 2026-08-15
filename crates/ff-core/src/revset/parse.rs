//! The revset parser: a Pratt parser over four binding powers, producing a
//! tree the resolver evaluates.
//!
//! Only four levels exist because the scanner already collapsed two of them:
//! `::` and `..` arrive as single tokens, so the parser never sees a `:` or a
//! `.` to bind. What remains is ancestry, complement, intersection, union, in
//! that order — tightest first, so `~a & b` is `(~a) & b` and `a | b & c` is
//! `a | (b & c)`, which is the reading every set language already trained
//! people to expect.
//!
//! The parser knows nothing about what a revision *means*, which function
//! names exist, or whether a pattern compiles. Those are the resolver's, and
//! keeping them there is what lets this half be tested without a repository.

use super::lex::{self, Token, TokenKind, lex};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A gitrevisions revision, verbatim. An opaque leaf here.
    Revision(String),
    /// `~x`
    Complement(Box<Expr>),
    /// `x & y`
    Intersection(Box<Expr>, Box<Expr>),
    /// `x | y`
    Union(Box<Expr>, Box<Expr>),
    /// `::x`, `x::`, `x::y`. An absent endpoint is the unbounded form — the
    /// parser records which one was written and leaves the default to the
    /// resolver, which is the half that knows what the repository's roots and
    /// heads are.
    DagRange {
        from: Option<Box<Expr>>,
        to: Option<Box<Expr>>,
    },
    /// `..x`, `x..`, `x..y`, with the same absent-endpoint rule.
    Range {
        from: Option<Box<Expr>>,
        to: Option<Box<Expr>>,
    },
    /// `name(args)`. Unknown names are the resolver's to refuse: a front end
    /// that owned the list would have to be edited every time a function earns
    /// its existence.
    Function { name: String, args: Vec<Arg> },
}

/// One argument to a function. A pattern's *kind* is recognized here and
/// nowhere else, because `glob:` only means a kind in argument position —
/// elsewhere it is an ordinary ref name with a colon in it. The scanner knows
/// the same prefixes for one narrower reason: they are where a quoted value
/// may open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Set(Expr),
    Pattern { kind: PatternKind, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    Exact,
    Glob,
    Substring,
    Regex,
}

impl PatternKind {
    /// Parallel to [`lex::PATTERN_PREFIXES`], which is the one place the
    /// spellings live — the scanner needs them to know where a quote may open.
    const ALL: [PatternKind; 4] = [
        PatternKind::Exact,
        PatternKind::Glob,
        PatternKind::Substring,
        PatternKind::Regex,
    ];

    /// Split a `kind:value` argument. Returns `None` for anything else,
    /// including a bare argument — an implicit kind is the resolver's call to
    /// make per function, not the grammar's.
    fn split(text: &str) -> Option<(PatternKind, &str)> {
        lex::PATTERN_PREFIXES
            .iter()
            .zip(PatternKind::ALL)
            .find_map(|(prefix, kind)| text.strip_prefix(prefix).map(|value| (kind, value)))
    }
}

/// Strip a quoted pattern value and unescape it. Quoting is the escape hatch
/// for a value carrying a metacharacter, never the calling convention, so both
/// spellings must arrive at the resolver identical: `exact:"main"` and
/// `exact:main` produce the same bytes and it cannot tell them apart.
fn unquote(value: &str) -> String {
    let Some(body) = value.strip_prefix('"') else {
        return value.to_string();
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        match ch {
            // Two escapes, matching the scanner. Anything else keeps its
            // backslash: `\d` belongs to the regex, not to this grammar.
            '\\' => match chars.next() {
                Some(c @ ('"' | '\\')) => out.push(c),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '"' => {
                // The scanner closed the run here. Text after it was never part
                // of the quoted value, so the argument was not one — take it
                // literally rather than inventing a reading for it.
                return if chars.next().is_some() {
                    value.to_string()
                } else {
                    out
                };
            }
            _ => out.push(ch),
        }
    }
    value.to_string()
}

const ANCESTRY: u8 = 40;
const COMPLEMENT: u8 = 30;
const INTERSECTION: u8 = 20;
const UNION: u8 = 10;

/// Parse a revset expression. The only entry point the resolver needs.
pub fn parse(src: &str) -> Result<Expr> {
    let tokens = lex(src)?;
    if tokens.is_empty() {
        return Err(Error::coded(
            "usage/revset-empty",
            "empty revision expression",
            vec!["ff log -r @".into()],
        ));
    }
    let mut p = Parser {
        src,
        tokens,
        pos: 0,
    };
    let expr = p.set()?;
    match p.peek() {
        None => Ok(expr),
        // `set` already refused everything that could start an operand, so
        // whatever is left is structure that never opened.
        Some(TokenKind::Close) => Err(p.unmatched("an extra `)` with no `(` to close")),
        _ => Err(p.unmatched("a `,` outside a function call")),
    }
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    /// Source text spanning tokens `[from, to)`, whitespace and all — so an
    /// error names what the user typed rather than a re-rendering of it.
    fn text(&self, from: usize, to: usize) -> &'a str {
        if to <= from || from >= self.tokens.len() {
            return "";
        }
        let start = self.tokens[from].span.start;
        let end = self.tokens[to.min(self.tokens.len()) - 1].span.end;
        &self.src[start..end]
    }

    /// Parse one complete set expression, then refuse an operand standing
    /// beside it with no operator between. This is where `a ~ b` lands: git
    /// requires `~` to be followed by digits or nothing, so the scanner has
    /// already split it into two operands and a complement, and the shape
    /// arrives here as an adjacency rather than as a mystery.
    fn set(&mut self) -> Result<Expr> {
        let from = self.pos;
        let expr = self.expr(0)?;
        match self.peek() {
            Some(TokenKind::Complement) => Err(self.infix_difference(from)),
            Some(TokenKind::Revision(_) | TokenKind::Open) => Err(self.juxtaposed(from)),
            _ => Ok(expr),
        }
    }

    fn expr(&mut self, min_bp: u8) -> Result<Expr> {
        let mut lhs = self.prefix()?;
        loop {
            let bp = match self.peek() {
                Some(TokenKind::DagRange | TokenKind::Range) => ANCESTRY,
                Some(TokenKind::And) => INTERSECTION,
                Some(TokenKind::Or) => UNION,
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            let op = self.tokens[self.pos].kind.clone();
            self.pos += 1;
            // Ancestry is the one level with a postfix form. `x::` is settled
            // by whether anything that could start an operand follows, which
            // is a question about the next token alone — no backtracking.
            if matches!(op, TokenKind::DagRange | TokenKind::Range) && !self.starts_operand() {
                lhs = range(&op, Some(lhs), None);
                continue;
            }
            let rhs = self.expr(bp + 1)?;
            lhs = match op {
                TokenKind::And => Expr::Intersection(Box::new(lhs), Box::new(rhs)),
                TokenKind::Or => Expr::Union(Box::new(lhs), Box::new(rhs)),
                op => range(&op, Some(lhs), Some(rhs)),
            };
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr> {
        let Some(kind) = self.peek().cloned() else {
            return Err(self.expected_expression());
        };
        match kind {
            TokenKind::Revision(text) => {
                self.pos += 1;
                if self.peek() == Some(&TokenKind::Open) && is_identifier(&text) {
                    self.pos += 1;
                    self.call(text)
                } else {
                    Ok(Expr::Revision(text))
                }
            }
            TokenKind::Open => {
                self.pos += 1;
                let inner = self.set()?;
                self.expect_close()?;
                Ok(inner)
            }
            TokenKind::Complement => {
                self.pos += 1;
                Ok(Expr::Complement(Box::new(self.expr(COMPLEMENT)?)))
            }
            op @ (TokenKind::DagRange | TokenKind::Range) => {
                self.pos += 1;
                let to = self.expr(ANCESTRY)?;
                Ok(range(&op, None, Some(to)))
            }
            _ => Err(self.expected_expression()),
        }
    }

    /// Arguments, with the opening `(` already consumed.
    fn call(&mut self, name: String) -> Result<Expr> {
        let mut args = Vec::new();
        if self.peek() != Some(&TokenKind::Close) {
            loop {
                args.push(self.arg()?);
                if self.peek() == Some(&TokenKind::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        self.expect_close()?;
        Ok(Expr::Function { name, args })
    }

    fn arg(&mut self) -> Result<Arg> {
        let from = self.pos;
        let expr = self.set()?;
        // A pattern is one revision token wearing a known kind prefix, and
        // nothing else: `glob:a|b` parsed as a union, so it is a union.
        if self.pos == from + 1
            && let Expr::Revision(text) = &expr
            && let Some((kind, value)) = PatternKind::split(text)
        {
            return Ok(Arg::Pattern {
                kind,
                value: unquote(value),
            });
        }
        Ok(Arg::Set(expr))
    }

    fn starts_operand(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                TokenKind::Revision(_)
                    | TokenKind::Open
                    | TokenKind::Complement
                    | TokenKind::DagRange
                    | TokenKind::Range
            )
        )
    }

    fn expect_close(&mut self) -> Result<()> {
        if self.peek() == Some(&TokenKind::Close) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unmatched("a `(` that never closes"))
        }
    }

    /// `a ~ b`. The deliberate absence of an infix difference is what produced
    /// this shape, so the error spends its one line teaching the spelling that
    /// exists rather than describing the token that surprised the parser.
    fn infix_difference(&mut self, from: usize) -> Error {
        let tilde = self.pos;
        let left = self.text(from, tilde);
        self.pos += 1;
        // Parse the right operand for its extent alone. A malformed one has a
        // better error of its own, and it wins.
        if let Err(err) = self.expr(COMPLEMENT) {
            return err;
        }
        let right = self.text(tilde + 1, self.pos);
        Error::coded(
            "usage/revset-adjacent-operands",
            format!("fufu has no infix difference; `{left} ~ {right}` is `{left} & ~{right}`"),
            vec![format!("ff log -r \"{left} & ~{right}\"")],
        )
    }

    fn juxtaposed(&mut self, from: usize) -> Error {
        let left = self.text(from, self.pos);
        let right = self.text(self.pos, self.pos + 1);
        Error::coded(
            "usage/revset-adjacent-operands",
            format!("`{left}` and `{right}` stand side by side with no operator between them"),
            vec![
                format!("ff log -r \"{left} & {right}\""),
                format!("ff log -r \"{left} | {right}\""),
            ],
        )
    }

    fn expected_expression(&self) -> Error {
        let found = match self.peek() {
            Some(kind) => format!("found {}", describe(kind)),
            None => "the expression ended".to_string(),
        };
        Error::coded(
            "usage/revset-expected-expression",
            format!("expected a revision in `{}`, {found}", self.src),
            vec![],
        )
    }

    fn unmatched(&self, what: &str) -> Error {
        Error::coded(
            "usage/revset-unbalanced-parens",
            format!("`{}` has {what}", self.src),
            vec![],
        )
    }
}

fn range(op: &TokenKind, from: Option<Expr>, to: Option<Expr>) -> Expr {
    let from = from.map(Box::new);
    let to = to.map(Box::new);
    match op {
        TokenKind::DagRange => Expr::DagRange { from, to },
        _ => Expr::Range { from, to },
    }
}

/// Only a plain name can be called, so `main~2(x)` is an adjacency rather than
/// a call to something named `main~2`.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Revision(text) => format!("`{text}`"),
        TokenKind::DagRange => "`::`".into(),
        TokenKind::Range => "`..`".into(),
        TokenKind::Complement => "`~`".into(),
        TokenKind::And => "`&`".into(),
        TokenKind::Or => "`|`".into(),
        TokenKind::Open => "`(`".into(),
        TokenKind::Close => "`)`".into(),
        TokenKind::Comma => "`,`".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rev(text: &str) -> Expr {
        Expr::Revision(text.into())
    }

    fn boxed(expr: Expr) -> Option<Box<Expr>> {
        Some(Box::new(expr))
    }

    fn id_of(src: &str) -> String {
        parse(src).expect_err("refused").id().to_string()
    }

    #[test]
    fn whitespace_is_irrelevant_to_meaning() {
        for (tight, spaced) in [
            ("a&~b", " a  &  ~ b "),
            ("a|b&c", "a | b & c"),
            ("(a|b)&c", " ( a | b ) & c "),
            ("::a", ":: a"),
            ("a::b", "a :: b"),
            ("latest(a,b)", "latest ( a , b )"),
            ("~a::b", "~ a :: b"),
        ] {
            assert_eq!(
                parse(tight).expect("tight parses"),
                parse(spaced).expect("spaced parses"),
                "{tight} vs {spaced}"
            );
        }
    }

    #[test]
    fn suffixed_revisions_are_leaves() {
        assert_eq!(parse("main~2").expect("parses"), rev("main~2"));
        assert_eq!(
            parse("~main").expect("parses"),
            Expr::Complement(Box::new(rev("main")))
        );
        assert_eq!(
            parse("~main~2").expect("parses"),
            Expr::Complement(Box::new(rev("main~2")))
        );
        assert_eq!(
            parse("a&~b").expect("parses"),
            Expr::Intersection(
                Box::new(rev("a")),
                Box::new(Expr::Complement(Box::new(rev("b"))))
            )
        );
    }

    #[test]
    fn precedence_is_tightest_first() {
        assert_eq!(
            parse("a | b & c").expect("parses"),
            Expr::Union(
                Box::new(rev("a")),
                Box::new(Expr::Intersection(Box::new(rev("b")), Box::new(rev("c"))))
            )
        );
        assert_eq!(
            parse("~a & b").expect("parses"),
            Expr::Intersection(
                Box::new(Expr::Complement(Box::new(rev("a")))),
                Box::new(rev("b"))
            )
        );
        // Ancestry outranks complement, so `~` takes the whole range.
        assert_eq!(
            parse("~a::b").expect("parses"),
            Expr::Complement(Box::new(Expr::DagRange {
                from: boxed(rev("a")),
                to: boxed(rev("b")),
            }))
        );
        // Parentheses still override it.
        assert_eq!(
            parse("(a | b) & c").expect("parses"),
            Expr::Intersection(
                Box::new(Expr::Union(Box::new(rev("a")), Box::new(rev("b")))),
                Box::new(rev("c"))
            )
        );
    }

    #[test]
    fn every_ancestry_form() {
        assert_eq!(
            parse("::a").expect("parses"),
            Expr::DagRange {
                from: None,
                to: boxed(rev("a"))
            }
        );
        assert_eq!(
            parse("a::").expect("parses"),
            Expr::DagRange {
                from: boxed(rev("a")),
                to: None
            }
        );
        assert_eq!(
            parse("a::b").expect("parses"),
            Expr::DagRange {
                from: boxed(rev("a")),
                to: boxed(rev("b"))
            }
        );
        assert_eq!(
            parse("..a").expect("parses"),
            Expr::Range {
                from: None,
                to: boxed(rev("a"))
            }
        );
        assert_eq!(
            parse("a..").expect("parses"),
            Expr::Range {
                from: boxed(rev("a")),
                to: None
            }
        );
        assert_eq!(
            parse("a..b").expect("parses"),
            Expr::Range {
                from: boxed(rev("a")),
                to: boxed(rev("b"))
            }
        );
        // Postfix beside an operator: the next token cannot start an operand,
        // so the range closes and the intersection still binds.
        assert_eq!(
            parse("a:: & b").expect("parses"),
            Expr::Intersection(
                Box::new(Expr::DagRange {
                    from: boxed(rev("a")),
                    to: None
                }),
                Box::new(rev("b"))
            )
        );
    }

    #[test]
    fn calls_take_zero_one_or_several_arguments() {
        assert_eq!(
            parse("heads()").expect("parses"),
            Expr::Function {
                name: "heads".into(),
                args: vec![]
            }
        );
        assert_eq!(
            parse("latest(main)").expect("parses"),
            Expr::Function {
                name: "latest".into(),
                args: vec![Arg::Set(rev("main"))]
            }
        );
        assert_eq!(
            parse("coalesce(a, b, c)").expect("parses"),
            Expr::Function {
                name: "coalesce".into(),
                args: vec![Arg::Set(rev("a")), Arg::Set(rev("b")), Arg::Set(rev("c"))]
            }
        );
        // Nested, and an argument that is itself an expression.
        assert_eq!(
            parse("latest(heads(a) & ~b)").expect("parses"),
            Expr::Function {
                name: "latest".into(),
                args: vec![Arg::Set(Expr::Intersection(
                    Box::new(Expr::Function {
                        name: "heads".into(),
                        args: vec![Arg::Set(rev("a"))]
                    }),
                    Box::new(Expr::Complement(Box::new(rev("b"))))
                ))]
            }
        );
    }

    #[test]
    fn pattern_arguments_carry_their_kind() {
        for (src, kind, value) in [
            ("description(exact:fix)", PatternKind::Exact, "fix"),
            ("description(glob:fix*)", PatternKind::Glob, "fix*"),
            ("description(substring:fix)", PatternKind::Substring, "fix"),
            ("description(regex:fix.*)", PatternKind::Regex, "fix.*"),
        ] {
            assert_eq!(
                parse(src).expect("parses"),
                Expr::Function {
                    name: "description".into(),
                    args: vec![Arg::Pattern {
                        kind,
                        value: value.into()
                    }]
                },
                "{src}"
            );
        }
        // A bare argument stays a set: an implicit kind is per-function, and
        // functions are the resolver's.
        assert_eq!(
            parse("description(fix)").expect("parses"),
            Expr::Function {
                name: "description".into(),
                args: vec![Arg::Set(rev("fix"))]
            }
        );
        // Outside argument position the same text is an ordinary ref name.
        assert_eq!(parse("glob:fix*").expect("parses"), rev("glob:fix*"));
    }

    fn pattern_value(src: &str) -> String {
        match parse(src).expect("parses") {
            Expr::Function { args, .. } => match args.into_iter().next() {
                Some(Arg::Pattern { value, .. }) => value,
                other => panic!("{src}: expected a pattern argument, got {other:?}"),
            },
            other => panic!("{src}: expected a call, got {other:?}"),
        }
    }

    #[test]
    fn quoting_carries_a_metacharacter_through_untouched() {
        // The values that motivated quoting: each contains a byte the scanner
        // would otherwise have read as an operator or a suffix.
        for (src, value) in [
            (r#"description(regex:"^fix")"#, "^fix"),
            (r#"description(substring:"fix bug")"#, "fix bug"),
            (r#"description(glob:"a&b|c(d),e~f^g")"#, "a&b|c(d),e~f^g"),
            (r#"description(exact:"say \"hi\"")"#, r#"say "hi""#),
            (r#"description(regex:"a\\b")"#, r"a\b"),
            (r#"description(exact:"")"#, ""),
            // Two escapes and no zoo: a regex keeps its own backslashes.
            (r#"description(regex:"\d+\.\d+")"#, r"\d+\.\d+"),
        ] {
            assert_eq!(pattern_value(src), value, "{src}");
        }
    }

    #[test]
    fn the_resolver_cannot_tell_which_spelling_was_used() {
        for (quoted, bare) in [
            (r#"description(exact:"main")"#, "description(exact:main)"),
            (r#"description(glob:"fix*")"#, "description(glob:fix*)"),
        ] {
            assert_eq!(
                parse(quoted).expect("quoted parses"),
                parse(bare).expect("bare parses"),
                "{quoted} vs {bare}"
            );
        }
        // And the unquoted form is untouched by any of this.
        assert_eq!(
            parse("description(glob:fix*)").expect("parses"),
            Expr::Function {
                name: "description".into(),
                args: vec![Arg::Pattern {
                    kind: PatternKind::Glob,
                    value: "fix*".into()
                }]
            }
        );
    }

    #[test]
    fn an_unclosed_quote_is_coded() {
        assert_eq!(
            id_of(r#"description(regex:"^fix)"#),
            "usage/revset-unterminated-quote"
        );
    }

    #[test]
    fn adjacency_teaches_the_missing_infix_difference() {
        let err = parse("a ~ b").expect_err("refused");
        assert_eq!(err.id(), "usage/revset-adjacent-operands");
        assert_eq!(
            err.to_string(),
            "fufu has no infix difference; `a ~ b` is `a & ~b`"
        );
        assert_eq!(err.exit_code(), 2);

        // The user's own operands, not placeholders.
        let err = parse("main ~ old@{1}").expect_err("refused");
        assert!(
            err.to_string().contains("`main & ~old@{1}`"),
            "{}",
            err.to_string()
        );

        // Inside a call, and inside parentheses, it is still the same fault.
        assert_eq!(id_of("latest(a ~ b)"), "usage/revset-adjacent-operands");
        assert_eq!(id_of("(a ~ b)"), "usage/revset-adjacent-operands");
    }

    #[test]
    fn juxtaposed_operands_name_both_ways_out() {
        let err = parse("a b").expect_err("refused");
        assert_eq!(err.id(), "usage/revset-adjacent-operands");
        assert_eq!(err.exits().len(), 2, "an intersection and a union");
    }

    #[test]
    fn refused_shorthands_reach_the_parser_as_themselves() {
        assert_eq!(id_of("x+"), "revset/deferred-descendants");
        assert_eq!(
            parse("x+").expect_err("refused").exit_code(),
            1,
            "a capability that is not here yet, not a command line that was wrong"
        );
        assert_eq!(id_of("x-"), "usage/revset-parent-shorthand");
    }

    #[test]
    fn structural_faults_are_coded() {
        assert_eq!(id_of(""), "usage/revset-empty");
        assert_eq!(id_of("   "), "usage/revset-empty");
        assert_eq!(id_of("a &"), "usage/revset-expected-expression");
        assert_eq!(id_of("&"), "usage/revset-expected-expression");
        assert_eq!(id_of("~"), "usage/revset-expected-expression");
        assert_eq!(id_of("f(a,)"), "usage/revset-expected-expression");
        assert_eq!(id_of("(a"), "usage/revset-unbalanced-parens");
        assert_eq!(id_of("a)"), "usage/revset-unbalanced-parens");
        assert_eq!(id_of("a, b"), "usage/revset-unbalanced-parens");
    }
}
