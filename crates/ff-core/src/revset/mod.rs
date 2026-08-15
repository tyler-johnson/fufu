//! Revision sets — the language every verb taking a revision routes through.
//!
//! gitrevisions' revision grammar entire — its symbols and its suffixes,
//! delegated to gix and never reinterpreted — with fufu's own set algebra
//! layered around it. The split matters: ranges here are the set language's,
//! not git's, which is why `a..b` is inherited in spelling only and `a...b` is
//! refused outright. This module is the front end: text in, tree out, no
//! repository touched. What a revision *denotes* belongs to the resolver,
//! which is why a revision token is opaque here — the scanner recognizes its
//! full extent and hands the bytes along without reading them.
//!
//! Three jj spellings are absent by rule rather than by oversight, because
//! each is something gitrevisions already says: `x-` is `x^`, `x ~ y` is
//! `x & ~y`, and `x+` names a walk with no index behind it. All three are
//! recognized anyway, so typing one is taught rather than mystified.

pub mod lex;
pub mod parse;

#[cfg(test)]
mod prop;

pub use lex::{Token, TokenKind, lex};
pub use parse::{Arg, Expr, PatternKind, parse};
