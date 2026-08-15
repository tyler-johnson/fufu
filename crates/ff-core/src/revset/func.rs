//! The function registry: one table of name, arity, and argument kinds, and
//! one place that turns a call into a plan.
//!
//! Everything DESIGN names is in the table, including the four functions that
//! belong to the other address space. Recognized-and-refused beats
//! unrecognized: a reader who types `session(x)` against revisions has the
//! right idea and the wrong space, and being told which verb reads operations
//! is worth more than being told the name does not exist.

use crate::error::{Error, Result};

use super::eval::{self, Plan};
use super::parse::{Arg, Expr, PatternKind};
use super::pattern::Pattern;

/// Which commit field a predicate reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Description,
    Author,
}

/// What one argument position accepts. A set where a pattern belongs (or the
/// reverse) is a signature error, not a coercion — guessing there would make
/// `description(main)` mean two different things on two different days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Set,
    Pattern,
}

impl Kind {
    fn spelled(self) -> &'static str {
        match self {
            Kind::Set => "<set>",
            Kind::Pattern => "<pattern>",
        }
    }
}

struct Signature {
    name: &'static str,
    args: &'static [Kind],
}

impl Signature {
    fn spelled(&self) -> String {
        let args: Vec<&str> = self.args.iter().map(|k| k.spelled()).collect();
        format!("{}({})", self.name, args.join(", "))
    }
}

const FUNCTIONS: &[Signature] = &[
    Signature {
        name: "latest",
        args: &[Kind::Set],
    },
    Signature {
        name: "heads",
        args: &[Kind::Set],
    },
    Signature {
        name: "roots",
        args: &[Kind::Set],
    },
    Signature {
        name: "description",
        args: &[Kind::Pattern],
    },
    Signature {
        name: "author",
        args: &[Kind::Pattern],
    },
];

/// The op-space functions, named in DESIGN and evaluated by the op-space
/// backend. Listed here so revision space refuses them by name.
const OP_SPACE: [&str; 4] = ["base", "on_branch", "session", "kind"];

/// Bind one call. Every refusal a function can raise happens here, at bind
/// time, so an unknown name costs no more than a misspelled branch does.
pub(super) fn bind(repo: &gix::Repository, name: &str, args: &[Arg]) -> Result<Plan> {
    if OP_SPACE.contains(&name) {
        return Err(wrong_space(name));
    }
    if name == "descendants" {
        return Err(deferred_descendants());
    }
    let Some(sig) = FUNCTIONS.iter().find(|s| s.name == name) else {
        return Err(unknown_function(name));
    };
    if args.len() != sig.args.len() {
        return Err(arity(sig, args.len()));
    }

    Ok(match name {
        "latest" => Plan::Latest(Box::new(set_arg(repo, sig, &args[0])?)),
        "heads" => Plan::Extremes {
            of: Box::new(set_arg(repo, sig, &args[0])?),
            heads: true,
        },
        "roots" => Plan::Extremes {
            of: Box::new(set_arg(repo, sig, &args[0])?),
            heads: false,
        },
        "description" => Plan::Scan {
            field: Field::Description,
            pattern: pattern_arg(sig, &args[0])?,
        },
        _ => Plan::Scan {
            field: Field::Author,
            pattern: pattern_arg(sig, &args[0])?,
        },
    })
}

fn set_arg(repo: &gix::Repository, sig: &Signature, arg: &Arg) -> Result<Plan> {
    match arg {
        Arg::Set(expr) => eval::plan_of(repo, expr),
        Arg::Pattern { .. } => Err(arity_kind(sig)),
    }
}

/// A bare argument's implicit kind is the calling function's call, and both
/// predicates here call it `substring:` — the kind a reader who typed
/// `description(fix)` almost certainly meant.
fn pattern_arg(sig: &Signature, arg: &Arg) -> Result<Pattern> {
    match arg {
        Arg::Pattern { kind, value } => Pattern::new(*kind, value.clone()),
        Arg::Set(Expr::Revision(text)) => Pattern::new(PatternKind::Substring, text.clone()),
        // A whole expression where a pattern belongs is a signature error:
        // `description(a | b)` has no reading a predicate could take.
        Arg::Set(_) => Err(arity_kind(sig)),
    }
}

/// The one function whose absence is a capability rather than a spelling:
/// `descendants(x, depth)` is depth-bounded, which needs an index git has no
/// child edges to build. The unbounded form exists, so the message names it —
/// the same refusal `x+` gets in the scanner, for the same reason.
fn deferred_descendants() -> Error {
    Error::coded(
        "revset/deferred-descendants",
        "no `descendants(x, depth)` yet: children are a reverse walk with no index \
         behind them. `x::` is the descendant set that does exist, unbounded",
        vec!["ff log -r \"x::\"".into()],
    )
}

fn unknown_function(name: &str) -> Error {
    let names: Vec<&str> = FUNCTIONS.iter().map(|s| s.name).collect();
    Error::coded(
        "usage/revset-unknown-function",
        format!(
            "no revset function named `{name}`; revisions have {}",
            names.join(", ")
        ),
        vec!["ff log -r \"latest(main)\"".into()],
    )
}

fn arity(sig: &Signature, got: usize) -> Error {
    Error::coded(
        "usage/revset-arity",
        format!(
            "{} takes {} argument(s), not {got}",
            sig.spelled(),
            sig.args.len()
        ),
        vec![format!("ff log -r \"{}\"", sig.spelled())],
    )
}

fn arity_kind(sig: &Signature) -> Error {
    Error::coded(
        "usage/revset-arity",
        format!("{} was given an argument of the wrong kind", sig.spelled()),
        vec![format!("ff log -r \"{}\"", sig.spelled())],
    )
}

/// The two spaces share one grammar and not one vocabulary, so the message
/// has to say which space the name belongs to and which verb reads it.
fn wrong_space(name: &str) -> Error {
    Error::coded(
        "usage/revset-wrong-space",
        format!(
            "`{name}()` reads operations, and `-r` here takes revisions; the two address \
             spaces share a grammar, not a vocabulary"
        ),
        vec![format!("ff op log -r \"{name}(...)\"")],
    )
}
