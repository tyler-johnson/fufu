//! The writing half of the engine [`crate::futures::probe`] simulates: it
//! re-parents a range of commits — replaying them by three-way merge when the
//! change moves the target's tree or the range's floor onto a different base —
//! and writes the rewritten objects, moving no refs of its own. Every rewrite
//! verb, reword today and absorb and lift later, aims here rather than
//! forking its own commit-writing logic.

use std::collections::{HashMap, HashSet};

use gix::bstr::{BString, ByteSlice};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ops::record::RefTransition;

/// The stem of the label on the *ours* side of a chain's conflict markers:
/// the replay as it stands, everything before this step already folded in.
/// Each step appends its own `(k/n)`, exactly as the theirs side does, so a
/// file collecting more than one block wears no two identical marker lines —
/// a reader knows which step they are looking at without scrolling to the
/// closer, and nothing downstream has to tell two anchors apart by position.
const CHAIN_OURS: &str = "the rewrite so far";

/// One commit's old→new identity, full shas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewrite {
    pub old: String,
    pub new: String,
}

/// A commit a rewrite did not write: its tree matched its new first parent's,
/// so it introduces nothing, and fufu writes no empty commit. The old
/// identity is kept so the verb can name what it removed — a drop is
/// announced, never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dropped {
    /// The commit that was not rewritten, full sha.
    pub old: String,
    /// Its subject, so a report can say what went.
    pub subject: String,
}

/// What changes about the named commit. Absorb and lift add their variants
/// here rather than forking the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Message(String),
    /// The target's new tree, already computed by the caller, and optionally
    /// its new message. Descendants are replayed onto it rather than merely
    /// re-parented. The message rides along because an amend can change both
    /// at once — a session that edits a commit's content *and* rewords it is
    /// one act, and splitting it across two `Change`s would land one half.
    Tree {
        tree: gix::ObjectId,
        message: Option<BString>,
    },
    /// The target's new first parent. Unlike the other two, this replays the
    /// target itself rather than only its descendants — which is what moves
    /// a range's floor onto a different base.
    Onto(gix::ObjectId),
}

/// The result of re-parenting `target..tip` after applying a [`Change`] to
/// `target`.
#[derive(Debug, Clone)]
pub struct RewritePlan {
    /// Every rewritten commit, oldest-first, target first.
    pub rewrites: Vec<Rewrite>,
    /// Commits the rewrite dropped rather than wrote, oldest-first.
    pub dropped: Vec<Dropped>,
    /// Local heads sitting inside the rewritten range, sorted by ref name.
    pub carried: Vec<RefTransition>,
    pub new_tip: gix::ObjectId,
}

/// Branch metadata the landing clears as part of its own operation, so one
/// `ff undo` takes a whole resolution back rather than leaving half of it
/// behind. `None` for an ordinary invocation, which clears nothing.
#[derive(Debug, Default, Clone)]
pub struct Clearing {
    pub branch: String,
    pub held: Option<crate::held::Held>,
    pub resolve: Option<crate::held::Resolve>,
}

/// The trees a landing already knows. Empty for an ordinary invocation: every
/// commit merges its own way. Non-empty when a resolution is landing, where
/// `chain` has already worked out what each rewritten commit should carry and
/// the merge has nothing left to decide.
#[derive(Debug, Default, Clone)]
pub struct Decided {
    pub trees: HashMap<gix::ObjectId, gix::ObjectId>,
    pub clearing: Option<Clearing>,
}

impl Decided {
    pub fn none() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }
}

/// Apply `change` to `target` and re-parent or replay every commit between it
/// and `tip`. Writes commit objects; moves no refs.
pub fn plan(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
    now: i64,
) -> Result<RewritePlan> {
    plan_with(repo, target, tip, change, now, &HashMap::new())
}

/// `plan`, with some commits' new trees decided in advance. A commit present
/// in `trees` skips the three-way merge and takes the tree it is given, which
/// is how a resolved held rewrite lands: `chain` computed those trees with the
/// user's edits folded into the steps that owned them, so the merge has
/// nothing left to decide.
pub fn plan_with(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
    now: i64,
    trees: &HashMap<gix::ObjectId, gix::ObjectId>,
) -> Result<RewritePlan> {
    let Range { ordered, affected } = range_of(repo, target, tip)?;

    // 5. Rewrite the affected commits. A tree-moving change can conflict, so
    // a dry run against an in-memory object store must pass first: it raises
    // the conflict or the merge-commit refusal, writes nothing, and only then
    // does the real pass run. A message change moves no tree, so it skips
    // the dry run and keeps today's single pass.
    let Replayed {
        rewrites,
        dropped,
        map,
    } = match change {
        Change::Message(_) => replay(repo, &ordered, &affected, target, change, now, trees)?,
        Change::Tree { .. } | Change::Onto(_) => {
            let memory = repo.clone().with_object_memory();
            replay(&memory, &ordered, &affected, target, change, now, trees)?;
            replay(repo, &ordered, &affected, target, change, now, trees)?
        }
    };

    // 6. Local heads inside the range that actually moved.
    let mut carried: Vec<RefTransition> = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed("refs/heads/").map_err(Error::repo)?;
    for reference in iter {
        let reference = reference.map_err(|err| {
            Error::coded(
                "op/unreadable",
                format!("ref iteration failed: {err}"),
                vec![],
            )
        })?;
        let Some(old_id) = reference.target().try_id().map(|id| id.to_owned()) else {
            continue; // symbolic heads are skipped silently
        };
        if let Some(&new_id) = map.get(&old_id)
            && new_id != old_id
        {
            carried.push(RefTransition {
                name: reference.name().as_bstr().to_string(),
                old: Some(old_id.to_string()),
                new: Some(new_id.to_string()),
            });
        }
    }
    carried.sort_by(|a, b| a.name.cmp(&b.name));

    // 7. The new tip.
    let new_tip = *map
        .get(&tip)
        .ok_or_else(|| Error::msg(format!("{tip} was not rewritten: not in the affected set")))?;

    Ok(RewritePlan {
        rewrites,
        dropped,
        carried,
        new_tip,
    })
}

/// The range a rewrite covers: every commit from `target` to `tip`, ordered
/// oldest-first, and which of them the rewrite touches. Shared by `plan` and
/// `chain`, which have to agree on both or their step numbering diverges.
struct Range {
    ordered: Vec<gix::ObjectId>,
    affected: HashSet<gix::ObjectId>,
}

fn range_of(repo: &gix::Repository, target: gix::ObjectId, tip: gix::ObjectId) -> Result<Range> {
    // 1. Collect the range, boundary-walked from `tip` and bounded by
    // `target`'s parents. A root `target` has no parents, so the boundary is
    // empty and the range is all of `tip`'s ancestry.
    let target_obj = repo.find_object(target).map_err(Error::repo)?;
    let target_commit_ref =
        gix::objs::CommitRef::from_bytes(&target_obj.data).map_err(Error::repo)?;
    let boundary: Vec<gix::ObjectId> = target_commit_ref
        .parents
        .iter()
        .map(|hex| gix::ObjectId::from_hex(hex).map_err(Error::repo))
        .collect::<Result<_>>()?;

    let walk = repo
        .rev_walk(Some(tip))
        .with_boundary(boundary.iter().copied())
        .all()
        .map_err(Error::repo)?;
    let mut range: HashSet<gix::ObjectId> = HashSet::new();
    let mut parents_of: HashMap<gix::ObjectId, Vec<gix::ObjectId>> = HashMap::new();
    for info in walk {
        let info = info.map_err(Error::repo)?;
        let parents: Vec<gix::ObjectId> = info.parent_ids.iter().copied().collect();
        range.insert(info.id);
        parents_of.insert(info.id, parents);
    }

    // 2. `target` must be in range.
    if !range.contains(&target) {
        let target_short = crate::sha::short_oid(target);
        let tip_short = crate::sha::short_oid(tip);
        return Err(Error::coded(
            "rewrite/not-in-history",
            format!(
                "{target_short} is not in the history of {tip_short}: there is nothing to \
                 rewrite there"
            ),
            vec!["ff log".into(), "ff log -r <rev>".into()],
        ));
    }

    // 3. Order the range oldest-first, parents before children: iterative
    // post-order DFS from `tip` over parents restricted to the range.
    let ordered = order_range(tip, &range, &parents_of);

    // 4. Mark which commits are affected: `target` is affected, and any
    // commit with an affected parent is affected.
    let mut affected: HashSet<gix::ObjectId> = HashSet::new();
    for &id in &ordered {
        let is_affected = id == target
            || parents_of
                .get(&id)
                .is_some_and(|parents| parents.iter().any(|p| affected.contains(p)));
        if is_affected {
            affected.insert(id);
        }
    }

    Ok(Range { ordered, affected })
}

/// The branch's tracking ref — the one git would fetch into, from
/// `branch.<name>.remote`/`branch.<name>.merge`: `refs/remotes/origin/<name>`
/// under the default fetch refspec. `None` when the branch has no upstream.
fn tracking_ref(repo: &gix::Repository, branch: &str) -> Result<Option<gix::refs::FullName>> {
    let full_name: gix::refs::FullName = format!("refs/heads/{branch}")
        .as_str()
        .try_into()
        .map_err(Error::repo)?;
    let Some(tracking) =
        repo.branch_remote_tracking_ref_name(full_name.as_ref(), gix::remote::Direction::Fetch)
    else {
        return Ok(None);
    };
    let tracking = tracking.map_err(Error::repo)?;
    Ok(Some(
        gix::refs::FullName::try_from(tracking.as_bstr()).map_err(Error::repo)?,
    ))
}

/// The short name of the tracking ref `published_count` measures against —
/// `origin/feature`. `None` when the branch has no upstream configured.
pub fn tracking_name(repo: &gix::Repository, branch: &str) -> Result<Option<String>> {
    Ok(tracking_ref(repo, branch)?.map(|tracking| tracking.shorten().to_string()))
}

/// How many of the commits the rewrite removed from the branch as they stood
/// — rewritten and dropped alike — the branch's remote-tracking ref already
/// contains: a published commit that is now gone is the one the remote will
/// miss hardest. Disclosure, not a guard: nothing here refuses a rewrite
/// because commits are published.
pub fn published_count(repo: &gix::Repository, branch: &str, plan: &RewritePlan) -> Result<usize> {
    let Some(tracking) = tracking_ref(repo, branch)? else {
        return Ok(0);
    };

    let mut tracking_ref = match repo.find_reference(tracking.as_ref()) {
        Ok(r) => r,
        Err(gix::reference::find::existing::Error::NotFound { .. }) => return Ok(0),
        Err(err) => return Err(Error::repo(err)),
    };
    let remote_tip = tracking_ref
        .peel_to_id_in_place()
        .map_err(Error::repo)?
        .detach();

    let mut count = 0usize;
    for sha in plan
        .rewrites
        .iter()
        .map(|r| r.old.as_bytes())
        .chain(plan.dropped.iter().map(|d| d.old.as_bytes()))
    {
        let Ok(old) = gix::ObjectId::from_hex(sha) else {
            continue;
        };
        if let Ok(bases) = repo.merge_bases_many(old, &[remote_tip])
            && bases.iter().any(|b| b.detach() == old)
        {
            count += 1;
        }
    }
    Ok(count)
}

/// Paths for a refusal message: the first three, "and N more" for the rest.
pub(crate) fn join_paths(paths: &[String]) -> String {
    if paths.len() <= 3 {
        paths.join(", ")
    } else {
        format!("{}, and {} more", paths[..3].join(", "), paths.len() - 3)
    }
}

/// What one replay pass produced. `map` is the load-bearing part: every
/// descendant reads it for its new parent, `plan` reads it for the carried
/// heads and for the new tip, and a dropped commit is exactly an entry
/// pointing at its parent rather than at a commit of its own.
struct Replayed {
    rewrites: Vec<Rewrite>,
    dropped: Vec<Dropped>,
    map: HashMap<gix::ObjectId, gix::ObjectId>,
}

/// Rewrite each affected commit in order: the target takes its new tree or
/// message, and every other affected commit is replayed onto its rewritten
/// first parent. Under a tree change a merge commit in the range is refused
/// before the first write.
fn replay(
    repo: &gix::Repository,
    ordered: &[gix::ObjectId],
    affected: &HashSet<gix::ObjectId>,
    target: gix::ObjectId,
    change: &Change,
    now: i64,
    trees: &HashMap<gix::ObjectId, gix::ObjectId>,
) -> Result<Replayed> {
    // Re-parenting a merge is unambiguous; replaying one is not — and
    // absorb never replays its target, so a merge target is exempt only
    // under Tree, not under Onto.
    if matches!(change, Change::Tree { .. } | Change::Onto(_)) {
        for &id in ordered {
            if (matches!(change, Change::Tree { .. }) && id == target) || !affected.contains(&id) {
                continue;
            }
            let commit = repo.find_object(id).map_err(Error::repo)?.into_commit();
            if commit.parent_ids().count() > 1 {
                let subject = commit.message().map_err(Error::repo)?.summary().to_string();
                return Err(Error::coded(
                    "rewrite/merge-in-range",
                    format!(
                        "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                         rewritten",
                        crate::sha::short_oid(id),
                        subject
                    ),
                    vec!["ff log".into()],
                ));
            }
        }
    }

    let committer = crate::refs::user_signature(repo, now)?;
    let mut map: HashMap<gix::ObjectId, gix::ObjectId> = HashMap::new();
    let mut rewrites: Vec<Rewrite> = Vec::new();
    let mut dropped: Vec<Dropped> = Vec::new();
    for &id in ordered {
        if !affected.contains(&id) {
            continue;
        }
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;

        let old_parents: Vec<gix::ObjectId> = commit_ref
            .parents
            .iter()
            .map(|hex| gix::ObjectId::from_hex(hex).map_err(Error::repo))
            .collect::<Result<_>>()?;
        let parents: Vec<gix::ObjectId> = if let Change::Onto(onto) = change
            && id == target
        {
            vec![*onto]
        } else {
            old_parents
                .iter()
                .map(|&old| map.get(&old).copied().unwrap_or(old))
                .collect()
        };
        let tree = if id == target {
            match change {
                Change::Message(_) => {
                    gix::ObjectId::from_hex(commit_ref.tree).map_err(Error::repo)?
                }
                // A decided landing has already worked out what the target
                // carries: the change's own tree is the fold as it was
                // computed, markers and all, and the decision is that fold
                // with the reader's fix in it. The decision wins.
                Change::Tree { tree, .. } => *trees.get(&id).unwrap_or(tree),
                Change::Onto(onto) => {
                    let base = match old_parents.first() {
                        Some(parent) => tree_of(repo, *parent)?,
                        None => gix::ObjectId::empty_tree(repo.object_hash()),
                    };
                    replayed_tree(repo, id, base, tree_of(repo, *onto)?, trees)?
                }
            }
        } else {
            let Some(old_parent0) = old_parents.first().copied() else {
                return Err(Error::msg(
                    "a non-target commit in the affected set has no first parent: internal \
                     ordering error",
                ));
            };
            let new_parent0 = *map.get(&old_parent0).unwrap_or(&old_parent0);
            replayed_tree(
                repo,
                id,
                tree_of(repo, old_parent0)?,
                tree_of(repo, new_parent0)?,
                trees,
            )?
        };
        let author = commit_ref.author.to_owned().map_err(Error::repo)?;
        let message: BString = if id == target {
            match change {
                Change::Message(text) => crate::close::normalize_message(text).into(),
                // The caller supplies a finished message — an existing
                // commit's, already in its final form — so it lands
                // verbatim, unlike the raw user input `Message` takes.
                Change::Tree {
                    message: Some(text),
                    ..
                } => text.clone(),
                Change::Tree { message: None, .. } | Change::Onto(_) => {
                    commit_ref.message.to_owned()
                }
            }
        } else {
            commit_ref.message.to_owned()
        };
        let mut extra_headers: Vec<(BString, BString)> = Vec::new();
        for (key, value) in &commit_ref.extra_headers {
            let name: &[u8] = key;
            if name == b"gpgsig" || name == b"gpgsig-sha256" {
                continue;
            }
            extra_headers.push(((*key).to_owned(), value.clone().into_owned()));
        }
        let encoding = commit_ref.encoding.map(|e| e.to_owned());

        // fufu writes no empty commit. A commit whose computed tree matches
        // its new first parent's introduces nothing, so it is not written:
        // `map` points it at that parent, and every descendant, every local
        // head sitting on it and `new_tip` follow on their own, because they
        // all read `map` and nothing else.
        //
        // Never a merge — collapsing one onto its first parent would erase
        // the other side of the history, which is not what "empty" means —
        // and never a root, which has no parent to collapse onto. Requiring
        // exactly one parent covers both. Never under `Change::Message`
        // either: a reword re-parents rather than replays, so every tree it
        // passes over is one it did not touch.
        if !matches!(change, Change::Message(_))
            && parents.len() == 1
            && tree == tree_of(repo, parents[0])?
        {
            map.insert(id, parents[0]);
            dropped.push(Dropped {
                old: id.to_string(),
                subject: subject(repo, id)?,
            });
            continue;
        }
        let commit = gix::objs::Commit {
            tree,
            parents: parents.into(),
            author,
            committer: committer.clone(),
            encoding,
            message,
            extra_headers,
        };
        let new_id = repo.write_object(&commit).map_err(Error::repo)?.detach();
        map.insert(id, new_id);
        rewrites.push(Rewrite {
            old: id.to_string(),
            new: new_id.to_string(),
        });
    }
    Ok(Replayed {
        rewrites,
        dropped,
        map,
    })
}

/// The new tree of a replayed commit, target or descendant. When `trees`
/// decides this commit's tree in advance, that tree is taken directly — this
/// is how a resolved held rewrite lands, the merge having nothing left to
/// decide. Otherwise: when its first parent's tree did not move, the commit's
/// own tree is carried unchanged and no merge runs at all — which is what
/// keeps a reword costing what it cost before — and otherwise its tree is
/// replayed onto the rewritten parent, and an unresolved merge refuses the
/// whole rewrite.
fn replayed_tree(
    repo: &gix::Repository,
    id: gix::ObjectId,
    base_tree: gix::ObjectId,
    ours_tree: gix::ObjectId,
    trees: &HashMap<gix::ObjectId, gix::ObjectId>,
) -> Result<gix::ObjectId> {
    if let Some(&given) = trees.get(&id) {
        return Ok(given);
    }
    if base_tree == ours_tree {
        return tree_of(repo, id);
    }
    let their = tree_of(repo, id)?;
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    let mut outcome = repo
        .merge_trees(base_tree, ours_tree, their, Default::default(), options)
        .map_err(Error::repo)?;
    let paths = crate::futures::unresolved(&outcome);
    if !paths.is_empty() {
        let commit = repo.find_object(id).map_err(Error::repo)?.into_commit();
        let subject = commit.message().map_err(Error::repo)?.summary().to_string();
        return Err(Error::coded(
            "held/rewrite-conflict",
            format!(
                "replaying {} \"{}\" over the rewrite conflicts in {}: nothing was rewritten",
                crate::sha::short_oid(id),
                subject,
                join_paths(&paths),
            ),
            vec!["ff status".into(), "ff log -r <rev>".into()],
        ));
    }
    Ok(outcome.tree.write().map_err(Error::repo)?.detach())
}

/// The tree of a commit, resolved through whichever repository handle is
/// given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// Iterative post-order DFS from `tip` over parents restricted to `range`,
/// yielding oldest-first (parents before children). No recursion — a deep
/// history must not blow the stack.
fn order_range(
    tip: gix::ObjectId,
    range: &HashSet<gix::ObjectId>,
    parents_of: &HashMap<gix::ObjectId, Vec<gix::ObjectId>>,
) -> Vec<gix::ObjectId> {
    let mut stack: Vec<(gix::ObjectId, bool)> = vec![(tip, false)];
    let mut seen: HashSet<gix::ObjectId> = HashSet::new();
    let mut ordered: Vec<gix::ObjectId> = Vec::new();
    while let Some((id, expanded)) = stack.pop() {
        if expanded {
            ordered.push(id);
            continue;
        }
        if seen.contains(&id) {
            continue;
        }
        seen.insert(id);
        stack.push((id, true));
        if let Some(parents) = parents_of.get(&id) {
            for &parent in parents {
                if range.contains(&parent) {
                    stack.push((parent, false));
                }
            }
        }
    }
    ordered
}

// ---------------------------------------------------------------------------
// The chain: a held rewrite replayed all the way through, with its conflicts
// carried forward as literal marker content rather than refused. Nothing is
// committed — this walks trees only — but the trees and blobs it writes are
// real, so `ff resolve` can check the last one out.
// ---------------------------------------------------------------------------

/// One unresolved region standing in a tree, and the step that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Index into the chain's `steps`.
    pub step: usize,
    pub path: String,
    /// The block's exact text — opening marker line through closing marker
    /// line, trailing newline included — so it can be found again verbatim in
    /// the tree the step produced and replaced there.
    pub block: String,
}

/// One step of a chain: a commit replayed onto everything before it, and
/// what it left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The commit being replayed, full sha.
    pub old: String,
    pub subject: String,
    /// The tree this step produced — carrying markers, when it conflicted.
    pub tree: gix::ObjectId,
    /// Paths this step left unresolved regions in, sorted and deduped.
    pub paths: Vec<String>,
}

/// The commit a chain stopped before, and the marks it would have written
/// over. Two conflicts on one region do not nest — they interleave, and the
/// earlier block stops being findable — so the chain stops rather than write
/// a tangle nobody can unpick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tangle {
    pub old: String,
    pub subject: String,
    pub path: String,
}

/// What a chain run produced: the replay simulated all the way through, with
/// unresolved regions carried forward as ordinary marker content rather than
/// stopping at the first one.
#[derive(Debug, Clone)]
pub struct Chain {
    /// The steps that ran, oldest-first.
    pub steps: Vec<Step>,
    /// The tree the last step produced, or the base's tree if none ran.
    pub tree: gix::ObjectId,
    /// Set when the chain stopped early.
    pub tangled: Option<Tangle>,
}

/// One resolved region, folded back into the step that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Index into `Chain::steps`.
    pub step: usize,
    pub path: String,
    /// The block's exact text as the step wrote it.
    pub block: String,
    /// What replaces it.
    pub with: String,
}

/// Replay `target..tip` under `change` without stopping at a conflict: each
/// step merges against the previous step's *result*, unresolved regions
/// carried forward as literal marker content, and a conflict a later commit
/// resolves anyway vanishes along the way. `resolutions` are applied to the
/// step that owns them, after that step merges and before the next one
/// replays over it, which is what makes the whole stack land clean from edits
/// made once at the end.
pub fn chain(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
    resolutions: &[Resolution],
) -> Result<Chain> {
    let Range { ordered, affected } = range_of(repo, target, tip)?;
    // The full size of the stack, even when the chain stops early: the label
    // tells the reader the size of the stack, not the size of this attempt.
    let n = ordered.iter().filter(|&&id| affected.contains(&id)).count();

    let start_cursor = match change {
        Change::Onto(onto) => tree_of(repo, *onto)?,
        Change::Tree { tree, .. } => *tree,
        // A reword moves no tree, so the cursor never stands in for a tree;
        // the empty tree only matters if no step runs at all.
        Change::Message(_) => gix::ObjectId::empty_tree(repo.object_hash()),
    };

    let mut cursor = start_cursor;
    let mut steps: Vec<Step> = Vec::new();
    let mut reported: HashSet<String> = HashSet::new();
    let mut tangled: Option<Tangle> = None;

    for &id in &ordered {
        if !affected.contains(&id) {
            continue;
        }
        let k = steps.len() + 1;
        let subject = subject(repo, id)?;

        // This step's tree and the regions it left unresolved. The target
        // under a tree change takes its new tree directly (no merge); a reword
        // carries every commit's own tree; everything else replays the commit
        // onto the cursor — the previous step's result.
        let (merged_tree, paths) = if id == target {
            match change {
                // The caller folded something into the target and handed
                // the result over. It can already carry marks — an absorb
                // whose fold conflicted hands over exactly that — so the
                // paths it changed are scanned like any other step's, or the
                // region would stand in every later tree with no step
                // claiming it.
                Change::Tree { tree, .. } => {
                    (*tree, marked_paths(repo, tree_of(repo, id)?, *tree)?)
                }
                Change::Message(_) => (tree_of(repo, id)?, Vec::new()),
                Change::Onto(onto) => {
                    let base = old_first_parent_tree(repo, id)?;
                    merged(repo, id, base, tree_of(repo, *onto)?, k, n, &subject)?
                }
            }
        } else {
            match change {
                Change::Message(_) => (tree_of(repo, id)?, Vec::new()),
                Change::Tree { .. } | Change::Onto(_) => {
                    let base = old_first_parent_tree(repo, id)?;
                    merged(repo, id, base, cursor, k, n, &subject)?
                }
            }
        };

        // Tangle check: every path any step so far reported unresolved — this
        // step's, plus every earlier one's — that still stands in this step's
        // tree must parse as clean fufu blocks. A fresh single conflict is
        // clean; a second one on the same region interleaves and is not.
        let mut to_check: Vec<String> = reported.iter().cloned().collect();
        to_check.extend(paths.iter().cloned());
        to_check.sort();
        to_check.dedup();
        let mut first_tangled: Option<&String> = None;
        for path in &to_check {
            if let Some(blob) = blob_of(repo, merged_tree, path)?
                && blocks(&blob).1
                && first_tangled.is_none()
            {
                first_tangled = Some(path);
            }
        }
        if let Some(path) = first_tangled {
            tangled = Some(Tangle {
                old: id.to_string(),
                subject,
                path: path.clone(),
            });
            break; // discard this step; the chain stops before it
        }

        // Fold this step's resolutions in, threading the tree, so the step's
        // stored tree is the one a landing pass can take straight.
        let idx = steps.len();
        let final_tree = apply_resolutions(repo, merged_tree, idx, resolutions)?;
        reported.extend(paths.iter().cloned());
        steps.push(Step {
            old: id.to_string(),
            subject,
            tree: final_tree,
            paths,
        });
        cursor = final_tree;
    }

    let tree = steps.last().map(|s| s.tree).unwrap_or(start_cursor);
    Ok(Chain {
        steps,
        tree,
        tangled,
    })
}

/// The first commit a rewrite cannot replay, and what stopped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The commit the replay stopped on. Always `At::Commit` from here — the
    /// open change is not a commit and no caller finds it by replaying one.
    pub at: crate::futures::At,
    pub paths: Vec<String>,
    /// How many commits the rewrite would have replayed in all, so a report
    /// can say "1 of 5" rather than leave the size of the stack unsaid.
    pub of: usize,
}

/// The verdict half of `plan`: the same replay, run for what it would say
/// rather than for the objects it would write. `None` means `plan` will not
/// conflict either, so a verb that asks this first never has to catch the
/// engine's refusal.
pub fn conflict(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
) -> Result<Option<Conflict>> {
    // A question costs no loose objects: the replay's trees and blobs are
    // written to a memory-backed store that is dropped with the answer.
    let memory = repo.clone().with_object_memory();
    let chain = chain(&memory, target, tip, change, &[])?;
    // The stopped chain still knows the commit it stopped before, and that
    // commit is part of the stack the report sizes.
    let of = chain.steps.len() + usize::from(chain.tangled.is_some());
    Ok(chain
        .steps
        .into_iter()
        .find(|step| !step.paths.is_empty())
        .map(|step| Conflict {
            at: crate::futures::At::Commit {
                id: step.old,
                subject: step.subject,
            },
            paths: step.paths,
            of,
        }))
}

/// The paths that differ between two trees and carry a fufu conflict block in
/// the second. Used where a tree arrives from outside the chain — a fold the
/// caller performed — so its marks are attributed rather than left ownerless.
fn marked_paths(
    repo: &gix::Repository,
    before: gix::ObjectId,
    after: gix::ObjectId,
) -> Result<Vec<String>> {
    if before == after {
        return Ok(Vec::new());
    }
    let mut changed: Vec<String> = Vec::new();
    let from = repo.find_tree(before).map_err(Error::repo)?;
    let to = repo.find_tree(after).map_err(Error::repo)?;
    from.changes()
        .map_err(Error::repo)?
        .for_each_to_obtain_tree(
            &to,
            |change| -> std::result::Result<_, std::convert::Infallible> {
                changed.push(change.location().to_string());
                Ok(gix::object::tree::diff::Action::Continue)
            },
        )
        .map_err(Error::repo)?;

    let mut marked: Vec<String> = Vec::new();
    for path in changed {
        if let Some(text) = blob_of(repo, after, &path)?
            && !blocks(&text).0.is_empty()
        {
            marked.push(path);
        }
    }
    marked.sort();
    marked.dedup();
    Ok(marked)
}

/// Every unresolved region standing in a chain's final tree, in path order
/// then in-file order, each tagged with the step that wrote it. This is the
/// list `ff resolve` shows and `ff done` attributes edits against.
pub fn regions(repo: &gix::Repository, chain: &Chain) -> Result<Vec<Region>> {
    let mut paths: Vec<String> = chain
        .steps
        .iter()
        .flat_map(|s| s.paths.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();

    let mut regions = Vec::new();
    for path in &paths {
        // A path some step conflicted on may have been resolved by a later
        // one; it simply yields no regions.
        let Some(blob) = blob_of(repo, chain.tree, path)? else {
            continue;
        };
        let (found, _tangled) = blocks(&blob);
        for block in found {
            regions.push(Region {
                step: block.step,
                path: path.clone(),
                block: block.text,
            });
        }
    }
    Ok(regions)
}

/// What a reader made of a resolution session, worked out per step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    /// Resolutions to hand straight back to `chain`. Never carries the last
    /// step: its tree is the resolved tree itself, so nothing about it needs
    /// attributing.
    pub resolutions: Vec<Resolution>,
    /// Regions the reader left alone, still carrying their markers.
    pub unresolved: Vec<Region>,
}

/// Work out which step each edit belongs to.
///
/// `chain.tree` is what `ff resolve` laid down; `resolved` is what the reader
/// made of it. Every marked region carries its owning step in its closing
/// label, so an edit landing inside a region belongs to that step, and its
/// resolution is whatever text now stands where the region stood.
///
/// An edit touching no region belongs to the last step and is therefore not
/// returned at all — the marker tree *is* the post-rewrite tip's tree, so an
/// unmarked edit is an edit to the final state, and attributing it earlier
/// would put content into a commit the reader never looked at.
pub fn attribute(
    repo: &gix::Repository,
    chain: &Chain,
    resolved: gix::ObjectId,
) -> Result<Attribution> {
    let mut paths: Vec<String> = chain
        .steps
        .iter()
        .flat_map(|s| s.paths.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();

    // The last step's tree is the resolved tree itself, so a resolution aimed
    // at it is a no-op and is dropped on the floor rather than returned.
    // A chain that stopped at a tangle has no such step: its tree is the
    // prefix's, not the post-rewrite tip's, so nothing the reader wrote is a
    // final state and every region it carries has to be attributed for real.
    let last: Option<usize> = chain
        .tangled
        .is_none()
        .then(|| chain.steps.len().saturating_sub(1));

    let mut resolutions: Vec<Resolution> = Vec::new();
    let mut unresolved: Vec<Region> = Vec::new();

    for path in &paths {
        // A path some step marked but the marker tree holds no blob at has
        // nothing to attribute.
        let Some(before) = blob_of(repo, chain.tree, path)? else {
            continue;
        };
        // The path absent from `resolved` means the reader deleted the file:
        // the last step's tree carries the deletion, and no earlier step can
        // express it as a text replacement, so nothing is attributed.
        let Some(after) = blob_of(repo, resolved, path)? else {
            continue;
        };
        let (found, _tangled) = blocks(&before);
        if found.is_empty() {
            continue;
        }
        let hunks = line_hunks(&before, &after);

        for block in &found {
            // Expand the block's range to absorb any hunk it overlaps, until
            // it stops growing: an edit that spilled past a marker line is
            // still that region's edit. A hunk already inside the range
            // widens it not at all, so the loop only runs while the range
            // actually grows — that is what makes it terminate.
            let mut range = block.lines.clone();
            loop {
                let mut next = range.clone();
                for (b, _) in &hunks {
                    if b.start < next.end && next.start < b.end {
                        next.start = next.start.min(b.start);
                        next.end = next.end.max(b.end);
                    }
                }
                if next == range {
                    break;
                }
                range = next;
            }

            // Map the expanded range into the resolved blob. A hunk lying
            // entirely before the range shifts it by its net delta; a hunk
            // the expansion absorbed shifts the range's far end by its net
            // delta. Together they bound the range's image, so the image is a
            // valid half-open line span within `after`.
            let mut delta_before = 0i64;
            let mut delta_inside = 0i64;
            for (b, a) in &hunks {
                let delta = (a.end as i64) - (a.start as i64) - (b.end as i64) + (b.start as i64);
                if b.end <= range.start {
                    delta_before += delta;
                } else if b.start < range.end {
                    delta_inside += delta;
                }
            }
            let start = (range.start as i64) + delta_before;
            let end = (range.end as i64) + delta_before + delta_inside;

            // The resolution text is the resolved blob's lines over the mapped
            // range, taken byte-for-byte (newline terminators included), so it
            // can stand in for the region exactly as the reader left it.
            let after_lines: Vec<&str> = after.split_inclusive('\n').collect();
            let with: String = after_lines[start as usize..end as usize].concat();

            // A surviving opener means the region was not finished, and text
            // byte-identical to the block means nobody touched it: both are
            // still unresolved. Otherwise it is a resolution — unless it
            // belongs to the last step, in which case it is dropped.
            if with.contains(OPENER) || with == block.text {
                unresolved.push(Region {
                    step: block.step,
                    path: path.clone(),
                    block: block.text.clone(),
                });
            } else if Some(block.step) != last {
                resolutions.push(Resolution {
                    step: block.step,
                    path: path.clone(),
                    block: block.text.clone(),
                    with,
                });
            }
        }
    }

    resolutions.sort_by(|a, b| a.path.cmp(&b.path).then(a.step.cmp(&b.step)));
    unresolved.sort_by(|a, b| a.path.cmp(&b.path).then(a.step.cmp(&b.step)));
    Ok(Attribution {
        resolutions,
        unresolved,
    })
}

/// The line-diff hunks between two texts: each change as the half-open line
/// range removed from `before` and the half-open line range inserted into
/// `after`, in strictly increasing order. `gix::diff::blob` re-exports
/// imara-diff, and its `InternedInput` tokenizes a `&str` into lines, so
/// these are line ranges, not byte or word ranges.
fn line_hunks(before: &str, after: &str) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    use gix::diff::blob::{Algorithm, intern::InternedInput};
    let input = InternedInput::new(before, after);
    let mut hunks: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    gix::diff::blob::diff(
        Algorithm::Histogram,
        &input,
        |b: std::ops::Range<u32>, a: std::ops::Range<u32>| {
            hunks.push((
                b.start as usize..b.end as usize,
                a.start as usize..a.end as usize,
            ));
        },
    );
    hunks
}

/// One non-trivial step: the commit's tree replayed onto `ours`. When the two
/// agree, no merge runs and the commit's own tree is carried — the same
/// short-circuit `replayed_tree` takes. Otherwise the three-way merge runs
/// with the chain's attribution labels, and the unresolved regions are the
/// step's paths.
fn merged(
    repo: &gix::Repository,
    id: gix::ObjectId,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    k: usize,
    n: usize,
    subject: &str,
) -> Result<(gix::ObjectId, Vec<String>)> {
    if base == ours {
        return Ok((tree_of(repo, id)?, Vec::new()));
    }
    let their = tree_of(repo, id)?;
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    let (ours_label, theirs) = chain_labels(subject, k, n);
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(ours_label.as_bytes().as_bstr()),
        other: Some(theirs.as_bytes().as_bstr()),
    };
    let mut outcome = repo
        .merge_trees(base, ours, their, labels, options)
        .map_err(Error::repo)?;
    let paths = crate::futures::unresolved(&outcome);
    let tree = outcome.tree.write().map_err(Error::repo)?.detach();
    Ok((tree, paths))
}

/// The pair of labels one chain step's merge writes, `(ours, theirs)`.
///
/// The theirs label is the whole of the attribution: it is the only thing that
/// survives into the tree saying which commit a marker block belongs to. A
/// subject containing a quote is written verbatim (not escaped) and is parsed
/// back by its outermost quotes — the first and the last on the line.
///
/// Shared, rather than private to `merged`, because a caller that folds a tree
/// of its own and hands the result to `chain` — an absorb, whose fold IS step
/// one — has to write the same labels: `blocks` only sees a block whose closer
/// carries a step, and a block nobody can attribute is a block that lands in a
/// commit.
pub(crate) fn chain_labels(subject: &str, k: usize, n: usize) -> (String, String) {
    (
        format!("{CHAIN_OURS} ({k}/{n})"),
        format!("rebasing \"{subject}\" ({k}/{n})"),
    )
}

/// How many commits a rewrite of `target..tip` replays — the `n` a chain
/// label's `(k/n)` counts against.
pub(crate) fn stack_size(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
) -> Result<usize> {
    let Range { ordered, affected } = range_of(repo, target, tip)?;
    Ok(ordered.iter().filter(|&&id| affected.contains(&id)).count())
}

/// The tree of a commit's old first parent, or the empty tree for a root.
fn old_first_parent_tree(repo: &gix::Repository, id: gix::ObjectId) -> Result<gix::ObjectId> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => {
            let parent = gix::ObjectId::from_hex(hex).map_err(Error::repo)?;
            tree_of(repo, parent)
        }
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// Fold every resolution that belongs to step `idx` into `tree`, one at a
/// time, threading the tree. A resolution whose block is not in the blob is
/// an error: the caller handed the engine a resolution the engine cannot
/// honor, and silently ignoring it would land the wrong content.
fn apply_resolutions(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    idx: usize,
    resolutions: &[Resolution],
) -> Result<gix::ObjectId> {
    let mut tree = tree;
    for res in resolutions.iter().filter(|r| r.step == idx) {
        let Some(blob) = blob_of(repo, tree, &res.path)? else {
            return Err(Error::msg(format!(
                "no such path to resolve at {}: the step's tree has no file there",
                res.path
            )));
        };
        if !blob.contains(&res.block) {
            return Err(Error::msg(format!(
                "no marker block to resolve at {}: the resolution does not match the tree",
                res.path
            )));
        }
        let updated = blob.replacen(&res.block, &res.with, 1);
        let new_blob = repo
            .write_object(gix::objs::Blob {
                data: updated.into_bytes(),
            })
            .map_err(Error::repo)?
            .detach();
        let kind = entry_kind(repo, tree, &res.path)?;
        let mut editor = repo.edit_tree(tree).map_err(Error::repo)?;
        editor
            .upsert(res.path.as_str(), kind, new_blob)
            .map_err(Error::repo)?;
        tree = editor.write().map_err(Error::repo)?.detach();
    }
    Ok(tree)
}

/// The kind of the entry at `path` in `tree`, so an amended blob keeps its
/// mode — an executable file does not lose its bit.
fn entry_kind(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    path: &str,
) -> Result<gix::objs::tree::EntryKind> {
    let tree = repo.find_tree(tree).map_err(Error::repo)?;
    let entry = tree
        .lookup_entry_by_path(path)
        .map_err(Error::repo)?
        .ok_or_else(|| Error::msg(format!("no entry at {path} in the tree")))?;
    Ok(entry.mode().kind())
}

/// The text of the blob at `path` in `tree`, or `None` when the path is not a
/// plain file in that tree.
fn blob_of(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Result<Option<String>> {
    let tree = repo.find_tree(tree).map_err(Error::repo)?;
    let Some(entry) = tree.lookup_entry_by_path(path).map_err(Error::repo)? else {
        return Ok(None);
    };
    let kind = entry.mode().kind();
    if kind != gix::objs::tree::EntryKind::Blob
        && kind != gix::objs::tree::EntryKind::BlobExecutable
    {
        return Ok(None);
    }
    let blob = repo.find_blob(entry.id().detach()).map_err(Error::repo)?;
    Ok(Some(String::from_utf8_lossy(&blob.data).into_owned()))
}

/// Whether the blob at `path` in `tree` carries a fufu opener line. The
/// check `ff done` runs over a landed chain's steps to catch a fix that
/// created a conflict further up the stack.
pub(crate) fn carries_markers(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    path: &str,
) -> Result<bool> {
    Ok(matches!(
        blob_of(repo, tree, path)?,
        Some(text) if text.contains(OPENER)
    ))
}

// The marker shapes. A **fufu block** is an opener line, any number of lines,
// then a closer line; the ours label is fixed and the closer carries the step
// that wrote it. Anything that does not match these exact shapes is foreign
// content — a file that legitimately contains conflict markers (a test
// fixture, a document about merging) must not be mistaken for fufu's own.
/// The stem every fufu opener starts with; each carries its own `(k/n)`
/// after it, so the whole line is matched by prefix.
const OPENER: &str = "<<<<<<< the rewrite so far";
const CLOSER_PREFIX: &str = ">>>>>>> rebasing \"";

/// A line that opens a fufu conflict block.
fn is_opener(line: &str) -> bool {
    line.trim_end_matches('\n').starts_with(OPENER)
}

/// A line that separates the ours and theirs halves of a conflict block.
fn is_separator(line: &str) -> bool {
    line.trim_end_matches('\n') == "======="
}

/// The 0-based step a closer line encodes, `None` when it is not a well-formed
/// fufu closer: the tail after the subject's closing quote must read
/// ` (<k>/<n>)` with both decimal and `k` at least one.
fn closer_step(line: &str) -> Option<usize> {
    let l = line.trim_end_matches('\n');
    let after = l.strip_prefix(CLOSER_PREFIX)?;
    // The subject sits between the first and the last quote; the step tail
    // follows the last one. A subject containing a quote is therefore taken
    // by its outermost quotes, not by escaping.
    let q = after.rfind('"')?;
    let tail = after[q..].strip_prefix('"')?;
    let tail = tail.strip_prefix(' ')?;
    let tail = tail.strip_prefix('(')?;
    let tail = tail.strip_suffix(')')?;
    let (k, _n) = tail.split_once('/')?;
    let k = k.parse::<usize>().ok()?;
    k.checked_sub(1)
}

/// A line that closes a fufu conflict block, with a parseable step.
fn is_closer(line: &str) -> bool {
    closer_step(line).is_some()
}

/// One fufu conflict block, located.
#[derive(Debug, Clone)]
struct Block {
    step: usize,
    text: String,
    /// The block's half-open line range in the text it was found in.
    lines: std::ops::Range<usize>,
}

/// Every fufu conflict block in `text`, in the order they appear, and whether
/// anything in the text is tangled. A tangle is a block the parser cannot
/// trust: an opener with no closer after it, a nested opener inside a block,
/// or more than one separator line inside a block. It is a value, not a
/// failure, so both are returned.
fn blocks(text: &str) -> (Vec<Block>, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut found: Vec<Block> = Vec::new();
    let mut tangled = false;

    let mut i = 0usize;
    while i < lines.len() {
        if !is_opener(lines[i]) {
            i += 1;
            continue;
        }
        // Scan forward to the first closer, or a nested opener, whichever
        // comes first.
        let mut j = i + 1;
        while j < lines.len() && !is_opener(lines[j]) && !is_closer(lines[j]) {
            j += 1;
        }
        if j < lines.len() && is_opener(lines[j]) {
            // A nested opener before any closer: tangle; resume past it.
            tangled = true;
            i = j + 1;
            continue;
        }
        if j < lines.len() && is_closer(lines[j]) {
            // More than one separator between the opener and the closer means
            // the region interleaved rather than nested.
            let separators = (i + 1..j).filter(|&m| is_separator(lines[m])).count();
            if separators > 1 {
                tangled = true;
            } else if let Some(step) = closer_step(lines[j]) {
                found.push(Block {
                    step,
                    text: lines[i..=j].concat(),
                    lines: i..j + 1,
                });
            }
            i = j + 1;
            continue;
        }
        // An opener with no closer at all: tangle.
        tangled = true;
        i += 1;
    }

    (found, tangled)
}
