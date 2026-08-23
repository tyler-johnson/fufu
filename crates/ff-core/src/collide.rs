//! Collide: the sideways axis. The other two axes measure one branch
//! against the base beneath it, or against the remote copy of itself — a
//! branch measured vertically. This one asks what those axes cannot: would
//! two branches, neither sitting beneath the other, collide with each other?
//!
//! The answer is symmetric — a three-way merge does not know which side is
//! "ours" — so each pair is judged exactly once, in the order the sides are
//! ranked. As with a futures probe, the merge runs inside an object-memory
//! clone, and `collide` writes nothing to the object database.

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

/// One pair, judged. `a` precedes `b` in the side order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub pairing: Pairing,
}

/// The answer: the sides ranked, every pair judged once, and the clear set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collisions {
    pub sides: Vec<Side>,
    pub pairs: Vec<Pair>,
    /// The names that are `Clear` against every name admitted before them,
    /// in side order. Greedy, not maximum: maximum independent set is
    /// NP-hard, so this is "first-come, all-clear", and a name skipped
    /// early is not re-admitted later because its blocker was not.
    pub clear: Vec<String>,
}

/// What to rank, and in what order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct CollideOptions {
    /// How many branches to rank in; `None` is every one. Ignored when
    /// `names` is non-empty.
    pub branches: Option<usize>,
    /// Explicit branches. Empty means "rank them by recency".
    pub names: Vec<String>,
}

/// A side with its ids still in hand: the report hex-encodes them, and the
/// judge reads them here.
struct SideRec {
    name: String,
    tip: gix::ObjectId,
    tree: gix::ObjectId,
    open: bool,
}

/// Would `a` and `b` collide, ranked by `opts`?
pub fn collide(repo: &gix::Repository, opts: &CollideOptions) -> Result<Collisions> {
    let candidates = candidates(repo, opts)?;
    let recs = resolve_sides(repo, &candidates)?;
    let pairs = judge(repo, &recs)?;
    let names: Vec<String> = recs.iter().map(|s| s.name.clone()).collect();
    let clear = clear_set(&names, &pairs);
    let sides: Vec<Side> = recs
        .iter()
        .map(|s| Side {
            name: s.name.clone(),
            tip: s.tip.to_string(),
            tree: s.tree.to_string(),
            open: s.open,
        })
        .collect();
    Ok(Collisions {
        sides,
        pairs,
        clear,
    })
}

/// The candidates in judgment order: the names given, in order, deduped
/// (first occurrence wins) and each required to exist; or every branch,
/// newest committer time first with the name breaking the tie, cut to the
/// rank — where `Some(0)` and `None` both mean every branch.
fn candidates(
    repo: &gix::Repository,
    opts: &CollideOptions,
) -> Result<Vec<(String, gix::ObjectId)>> {
    if !opts.names.is_empty() {
        let mut out = Vec::new();
        for name in &opts.names {
            if out.iter().any(|(n, _)| n == name) {
                continue;
            }
            match crate::refs::ref_target(repo, &format!("refs/heads/{name}"))? {
                Some(tip) => out.push((name.clone(), tip)),
                None => {
                    return Err(Error::coded(
                        "branch/not-found",
                        format!("no branch named {name}"),
                        vec![],
                    ));
                }
            }
        }
        return Ok(out);
    }

    // Unresolvable names drop out here: ranking is a read, and one ref it
    // cannot follow is no reason to fail the rest.
    let mut ranked: Vec<(String, gix::ObjectId, i64)> = Vec::new();
    for name in crate::switch::branch_names(repo)? {
        let Some(tip) = crate::refs::ref_target(repo, &format!("refs/heads/{name}"))? else {
            continue;
        };
        ranked.push((name, tip, committer_time(repo, tip)?));
    }
    // Newest tip first; equal times are a clock accident the name breaks.
    ranked.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    // `Some(0)` means every branch, the same wish `--all` spells out.
    if let Some(n) = opts.branches
        && n > 0
    {
        ranked.truncate(n);
    }
    Ok(ranked
        .into_iter()
        .map(|(name, tip, _)| (name, tip))
        .collect())
}

/// The committer time of a commit, seconds since the epoch.
fn committer_time(repo: &gix::Repository, id: gix::ObjectId) -> Result<i64> {
    Ok(repo
        .find_commit(id)
        .map_err(Error::repo)?
        .committer()
        .map_err(Error::repo)?
        .time()
        .map_err(Error::repo)?
        .seconds)
}

/// Each side's judgment tree: the tip's, unless the operation log holds a
/// different tree for the branch — then the open change's, with `open` set.
fn resolve_sides(
    repo: &gix::Repository,
    candidates: &[(String, gix::ObjectId)],
) -> Result<Vec<SideRec>> {
    let mut sides = Vec::new();
    for (name, tip) in candidates {
        let tip_tree = futures::tree_of(repo, *tip)?;
        let open = futures::open_tree(repo, name)?;
        let (tree, is_open) = match open {
            Some(t) if t != tip_tree => (t, true),
            _ => (tip_tree, false),
        };
        sides.push(SideRec {
            name: name.clone(),
            tip: *tip,
            tree,
            open: is_open,
        });
    }
    Ok(sides)
}

/// Every `i < j` over the sides, in that order.
fn judge(repo: &gix::Repository, sides: &[SideRec]) -> Result<Vec<Pair>> {
    let mut pairs = Vec::new();
    for i in 0..sides.len() {
        for j in (i + 1)..sides.len() {
            pairs.push(Pair {
                a: sides[i].name.clone(),
                b: sides[j].name.clone(),
                pairing: pairing(repo, &sides[i], &sides[j])?,
            });
        }
    }
    Ok(pairs)
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

/// A name in when it is `Clear` against every name already in, in side
/// order. `Collide` and `Unknown` both block admission: an unrelated
/// history is not a promise of safety.
fn clear_set(names: &[String], pairs: &[Pair]) -> Vec<String> {
    let mut admitted: Vec<String> = Vec::new();
    for name in names {
        let all_clear = admitted.iter().all(|other| {
            pairing_between(name, other, pairs).is_some_and(|p| matches!(p, Pairing::Clear))
        });
        if all_clear {
            admitted.push(name.clone());
        }
    }
    admitted
}

/// The verdict between two names, whichever end they sit on.
fn pairing_between<'a>(a: &str, b: &str, pairs: &'a [Pair]) -> Option<&'a Pairing> {
    pairs
        .iter()
        .find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a))
        .map(|p| &p.pairing)
}
