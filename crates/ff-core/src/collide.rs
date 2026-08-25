//! Collide: the sideways axis. The other two axes measure one branch
//! against the base beneath it, or against the remote copy of itself — a
//! branch measured vertically. This one asks what those axes cannot: would
//! two branches, neither sitting beneath the other, collide with each other?
//!
//! The answer is symmetric — a three-way merge does not know which side is
//! "ours" — which is why the verb takes two branches rather than an ours
//! and a theirs. As with a futures probe, the merge runs inside an
//! object-memory clone, and `collide` writes nothing to the object database.
//!
//! One pair is the whole verb. A set of branches that land on each other
//! is a scheduling question, and scheduling needs a queue, a notion of
//! what is already in flight, and something to claim with — none of which
//! fufu has. What fufu has is the verdict, and the verdict is what it
//! answers.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::futures;

/// How a pair of branches answers each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Pairing {
    /// A three-way merge of the two against their base leaves no conflicts.
    Clear,
    /// The merge would conflict, on exactly these paths.
    Collide { paths: Vec<String> },
    /// No base to merge against: the answer is refused, not guessed.
    Unknown { reason: futures::UnknownReason },
}

/// One side, as it stands: the tip, unless the open change holds a
/// different tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Side {
    pub name: String,
    /// The branch tip, lowercase hex.
    pub tip: String,
    /// The tree this side is judged on: the open change's when it differs
    /// from the tip's, else the tip's.
    pub tree: String,
    /// True when that tree is work the operation log holds, not the tip —
    /// the answer includes work that has not been committed.
    pub open: bool,
}

/// The answer: both sides as they were judged, and the verdict between
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collision {
    pub a: Side,
    pub b: Side,
    pub pairing: Pairing,
}

/// A side with its ids still in hand: the report hex-encodes them, and the
/// judge reads them here.
struct SideRec {
    name: String,
    tip: gix::ObjectId,
    tree: gix::ObjectId,
    open: bool,
}

impl SideRec {
    /// The reporting form, ids hex-encoded.
    fn report(&self) -> Side {
        Side {
            name: self.name.clone(),
            tip: self.tip.to_string(),
            tree: self.tree.to_string(),
            open: self.open,
        }
    }
}

/// Would `a` and `b` collide?
pub fn collide(repo: &gix::Repository, a: &str, b: &str) -> Result<Collision> {
    // A branch never conflicts with itself, so `Clear` here would be a true
    // answer to a question nobody meant to ask.
    if a == b {
        return Err(Error::coded(
            "usage/collide-same-branch",
            format!("{a} cannot collide with itself"),
            vec!["ff collide <a> <b>".into()],
        ));
    }
    let a = resolve_side(repo, a)?;
    let b = resolve_side(repo, b)?;
    let pairing = pairing(repo, &a, &b)?;
    Ok(Collision {
        a: a.report(),
        b: b.report(),
        pairing,
    })
}

/// One side's tip and the tree to judge it on: the open change's when the
/// operation log holds one that differs from the tip's, else the tip's.
/// The log is read rather than the worktree, so a branch checked out in
/// another bay — or nowhere at all — still answers.
fn resolve_side(repo: &gix::Repository, name: &str) -> Result<SideRec> {
    let Some(tip) = crate::refs::ref_target(repo, &format!("refs/heads/{name}"))? else {
        return Err(Error::coded(
            "branch/not-found",
            format!("no branch named {name}"),
            vec![],
        ));
    };
    let tip_tree = futures::tree_of(repo, tip)?;
    let (tree, open) = match futures::open_tree(repo, name)? {
        Some(open) if open != tip_tree => (open, true),
        _ => (tip_tree, false),
    };
    Ok(SideRec {
        name: name.to_string(),
        tip,
        tree,
        open,
    })
}

/// The verdict for one pair, judged from the common ground.
fn pairing(repo: &gix::Repository, a: &SideRec, b: &SideRec) -> Result<Pairing> {
    // All merge bases, not just the best one: with a criss-cross history a
    // single base misreads both sides.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(a.tip, &[b.tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Ok(Pairing::Unknown {
            reason: futures::UnknownReason::UnrelatedHistories,
        });
    }
    // The base is a commit; the merge wants a tree.
    let base_tree = futures::tree_of(repo, bases[0])?;
    let paths = futures::conflict_paths(repo, base_tree, a.tree, b.tree)?;
    if paths.is_empty() {
        Ok(Pairing::Clear)
    } else {
        Ok(Pairing::Collide { paths })
    }
}
