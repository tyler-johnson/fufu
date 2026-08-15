//! Seeded fuzz over the front end. Deterministic (fixed seed, hand-rolled
//! LCG) so a failure replays.
//!
//! Two properties, and the first is the one the scanner exists for: the count
//! of `Complement` tokens must equal the number of prefix-complement nodes the
//! generator wrote. Every leaf in the pool below wears gitrevisions suffixes,
//! several of them `~n`, so a regression that lets `main~2` lex as three
//! tokens fails on the first seed rather than on some later expression that
//! happened to combine badly.
//!
//! The second: whitespace between tokens is irrelevant to meaning. Each
//! expression is rendered twice — every token jammed together, and every token
//! separated — and both must parse to the tree the generator built. Rendering
//! is fully parenthesized on purpose: precedence has its own tests, and mixing
//! the two questions would let a precedence bug masquerade as a spacing one.

use super::lex::{TokenKind, lex};
use super::parse::{Arg, Expr, PatternKind, parse};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes constants; plenty for shape shuffling.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Every leaf carries a suffix that shares a byte with an operator, so the
/// scanner is forced to disagree with itself about that byte on every pass.
const LEAVES: &[&str] = &[
    "main",
    "@",
    "@-ish",
    "main~2",
    "HEAD~",
    "a^",
    "b^2",
    "x^{tree}",
    "y^{/fix}",
    "z^!",
    "w^@",
    "v^-1",
    "u@{upstream}",
    "t@{2}",
    "s@{2 days ago}",
    "0f1e2d3c",
    "refs/heads/dead",
    "origin/main",
    "HEAD:README.md",
];

/// One generated expression and its two renderings.
struct Node {
    expr: Expr,
    tight: String,
    spaced: String,
}

fn leaf(rng: &mut Lcg) -> Node {
    let text = LEAVES[rng.below(LEAVES.len() as u64) as usize];
    Node {
        expr: Expr::Revision(text.into()),
        tight: text.into(),
        spaced: text.into(),
    }
}

fn shape(rng: &mut Lcg, depth: u32, complements: &mut usize) -> Node {
    if depth == 0 {
        return leaf(rng);
    }
    match rng.below(8) {
        0..=2 => leaf(rng),
        3 => {
            *complements += 1;
            let inner = shape(rng, depth - 1, complements);
            Node {
                expr: Expr::Complement(Box::new(inner.expr)),
                tight: format!("(~{})", inner.tight),
                spaced: format!("( ~ {} )", inner.spaced),
            }
        }
        4 => {
            let op = ["&", "|", "::", ".."][rng.below(4) as usize];
            let lhs = shape(rng, depth - 1, complements);
            let rhs = shape(rng, depth - 1, complements);
            Node {
                expr: apply(op, lhs.expr, rhs.expr),
                tight: format!("({}{op}{})", lhs.tight, rhs.tight),
                spaced: format!("( {} {op} {} )", lhs.spaced, rhs.spaced),
            }
        }
        5 => {
            let (op, dag) = ancestry(rng);
            let to = shape(rng, depth - 1, complements);
            Node {
                expr: range(dag, None, Some(to.expr)),
                tight: format!("({op}{})", to.tight),
                spaced: format!("( {op} {} )", to.spaced),
            }
        }
        6 => {
            let (op, dag) = ancestry(rng);
            let from = shape(rng, depth - 1, complements);
            Node {
                expr: range(dag, Some(from.expr), None),
                tight: format!("({}{op})", from.tight),
                spaced: format!("( {} {op} )", from.spaced),
            }
        }
        _ => call(rng, depth, complements),
    }
}

fn call(rng: &mut Lcg, depth: u32, complements: &mut usize) -> Node {
    let name = ["latest", "heads", "roots", "description"][rng.below(4) as usize];
    let count = rng.below(3);
    let mut args = Vec::new();
    let mut tight = Vec::new();
    let mut spaced = Vec::new();
    for _ in 0..count {
        // A pattern argument now and then: it is a single revision token to
        // the scanner, so it belongs in the spacing property too.
        if rng.below(4) == 0 {
            let (kind, text, value) = pattern(rng);
            args.push(Arg::Pattern {
                kind,
                value: value.into(),
            });
            tight.push(text.to_string());
            spaced.push(text.to_string());
        } else {
            let node = shape(rng, depth - 1, complements);
            args.push(Arg::Set(node.expr));
            tight.push(node.tight);
            spaced.push(node.spaced);
        }
    }
    Node {
        expr: Expr::Function {
            name: name.into(),
            args,
        },
        tight: format!("{name}({})", tight.join(",")),
        spaced: format!("{name} ( {} )", spaced.join(" , ")),
    }
}

fn apply(op: &str, lhs: Expr, rhs: Expr) -> Expr {
    match op {
        "&" => Expr::Intersection(Box::new(lhs), Box::new(rhs)),
        "|" => Expr::Union(Box::new(lhs), Box::new(rhs)),
        "::" => range(true, Some(lhs), Some(rhs)),
        _ => range(false, Some(lhs), Some(rhs)),
    }
}

fn ancestry(rng: &mut Lcg) -> (&'static str, bool) {
    if rng.below(2) == 0 {
        ("::", true)
    } else {
        ("..", false)
    }
}

/// Both spellings, because a quoted value carries whitespace and operator
/// bytes and so belongs in the spacing property as much as a brace group does.
fn pattern(rng: &mut Lcg) -> (PatternKind, &'static str, &'static str) {
    match rng.below(6) {
        0 => (PatternKind::Exact, "exact:fix", "fix"),
        1 => (PatternKind::Glob, "glob:fix*", "fix*"),
        2 => (PatternKind::Substring, "substring:fix", "fix"),
        3 => (PatternKind::Regex, "regex:fix.*", "fix.*"),
        4 => (PatternKind::Substring, r#"substring:"fix bug""#, "fix bug"),
        _ => (PatternKind::Regex, r#"regex:"^a|b&c~d^e""#, "^a|b&c~d^e"),
    }
}

fn range(dag: bool, from: Option<Expr>, to: Option<Expr>) -> Expr {
    let from = from.map(Box::new);
    let to = to.map(Box::new);
    if dag {
        Expr::DagRange { from, to }
    } else {
        Expr::Range { from, to }
    }
}

#[test]
fn complements_are_counted_and_spacing_never_matters() {
    let mut rng = Lcg(0x5eed_f0f0);
    for _ in 0..400 {
        let mut complements = 0;
        let node = shape(&mut rng, 4, &mut complements);
        for src in [&node.tight, &node.spaced] {
            let emitted = lex(src)
                .unwrap_or_else(|err| panic!("lexes {src:?}: {err}"))
                .iter()
                .filter(|t| t.kind == TokenKind::Complement)
                .count();
            assert_eq!(
                emitted, complements,
                "a `~` inside a revision token is a suffix, never a complement: {src:?}"
            );
            assert_eq!(
                parse(src).unwrap_or_else(|err| panic!("parses {src:?}: {err}")),
                node.expr,
                "{src:?}"
            );
        }
    }
}
