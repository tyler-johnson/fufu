//! Revision sets — the language every verb taking a revision routes through.
//!
//! gitrevisions' revision grammar entire — its symbols and its suffixes,
//! delegated to gix and never reinterpreted — with fufu's own set algebra
//! layered around it. The split matters: ranges here are the set language's,
//! not git's, which is why `a..b` is inherited in spelling only and `a...b` is
//! refused outright. [`lex`] and [`parse`] are the front end: text in, tree
//! out, no repository touched. What a revision *denotes* belongs to
//! [`resolve`], which is why a revision token is opaque to the scanner — it
//! recognizes the token's full extent and hands the bytes along without
//! reading them.
//!
//! Three jj spellings are absent by rule rather than by oversight, because
//! each is something gitrevisions already says: `x-` is `x^`, `x ~ y` is
//! `x & ~y`, and `x+` names a walk with no index behind it. All three are
//! recognized anyway, so typing one is taught rather than mystified.
//!
//! **One grammar spans both address spaces.** [`lex`] and [`parse`] never
//! learn which one they are in — the difference is what a leaf denotes and
//! which functions exist, not how the text is shaped — so the front end here
//! serves [`Revset::evaluate`] over commits and [`Revset::evaluate_ops`] over
//! operations alike. The op-space back end is [`opspace`], and it is a
//! fraction of the size of this one because the live log is a chain: no DAG
//! to sort, no merge base to find, no child map to build. `base()` is the one
//! function that crosses, and it crosses in the direction that has somewhere
//! to land — operations in, commits out.
//!
//! The back end spends its error budget in the middle. Parsing is pure,
//! binding resolves every leaf and raises every refusal in O(leaves), and
//! evaluation is lazy — so a bad revset fails in microseconds on any
//! repository, and `-r '::@' -n 25` costs twenty-five pops at any depth.

pub mod lex;
pub mod parse;
pub mod resolve;

mod eval;
mod func;
mod opspace;
mod pattern;

#[cfg(test)]
mod prop;

pub use lex::{Token, TokenKind, lex};
pub use opspace::OpMember;
pub use parse::{Arg, Expr, PatternKind, parse};

use crate::error::{Error, Result};
use crate::ops::CommitId;

/// One member of a revision set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rev {
    /// The open change: commit-shaped, id not yet minted.
    Open,
    Commit(CommitId),
}

/// A parsed revision expression, ready to evaluate against any repository.
#[derive(Debug, Clone)]
pub struct Revset {
    src: String,
    expr: Expr,
}

/// A single-member result, plus the name the resolver actually used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub rev: Rev,
    /// The short branch name, when the whole expression was one revision leaf
    /// that canonicalized to `refs/heads/<name>`. `None` otherwise — a
    /// tracking ref included, so a caller reading this reads "a branch that
    /// exists here".
    pub name: Option<String>,
    /// The canonical ref that leaf resolved to, tracking refs and tags
    /// included. `None` under the same conditions `name` is, minus the
    /// local-branch narrowing.
    pub full_ref: Option<String>,
}

impl Revset {
    pub fn parse(src: &str) -> Result<Self> {
        Ok(Revset {
            src: src.to_string(),
            expr: parse(src)?,
        })
    }

    /// Every member, newest commit time first, lazily.
    pub fn evaluate<'r>(
        &self,
        repo: &'r gix::Repository,
    ) -> Result<Box<dyn Iterator<Item = Result<Rev>> + 'r>> {
        let bound = eval::bind(repo, &self.expr)?;
        Ok(Box::new(
            eval::run(repo, bound.plan)?.map(|m| m.map(|m| m.rev)),
        ))
    }

    /// Every operation the expression denotes, newest first, lazily.
    ///
    /// The same parsed expression, read against the other address space. The
    /// front end never learned which space it was in, because the difference
    /// is what a leaf denotes and which functions exist — not how the text is
    /// shaped. That is what "one grammar spans both spaces" buys.
    pub fn evaluate_ops<'r>(
        &self,
        repo: &'r gix::Repository,
    ) -> Result<Box<dyn Iterator<Item = Result<opspace::OpMember>> + 'r>> {
        opspace::evaluate(repo, &self.expr)
    }

    /// Exactly one member, or an error. Zero and many are different errors
    /// because they need different advice, and neither is ever resolved by
    /// picking one — disambiguation is a spelling the reader chooses.
    pub fn point(&self, repo: &gix::Repository) -> Result<Point> {
        let bound = eval::bind(repo, &self.expr)?;
        let mut members = eval::run(repo, bound.plan)?;
        let Some(first) = members.next().transpose()? else {
            return Err(empty_set(&self.src));
        };
        // Stopping at the second is the whole discipline: "many" is a fact
        // about two members, and proving it must not cost the rest of them.
        if members.next().transpose()?.is_some() {
            return Err(not_a_point(&self.src));
        }
        Ok(Point {
            rev: first.rev,
            name: bound.name,
            full_ref: bound.full_ref,
        })
    }
}

fn empty_set(src: &str) -> Error {
    Error::coded(
        "usage/revset-empty-set",
        format!("`{src}` matches no revision"),
        vec!["ff log".into(), "ff branch".into()],
    )
}

fn not_a_point(src: &str) -> Error {
    Error::coded(
        "usage/revset-not-a-point",
        format!("`{src}` matches more than one revision, and this takes exactly one"),
        vec![
            format!("ff log -r \"latest({src})\""),
            format!("ff log -r \"heads({src})\""),
        ],
    )
}
