use std::collections::{HashMap, HashSet};

use gix::bstr::BString;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ops::record::RefTransition;

/// One commit's old→new identity, full shas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewrite {
    pub old: String,
    pub new: String,
}

/// A commit a rewrite did not write. Either its tree matched its new first
/// parent's, so it introduces nothing, and fufu writes no empty commit; or
/// the base it was replayed onto already holds a commit with its change id,
/// so it is a stale copy of one the base has since rewritten, and replaying
/// it would at best drop it as empty and at worst conflict on content the
/// branch never touched. The old identity is kept so the verb can name what
/// it removed — a drop is announced, never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dropped {
    /// The commit that was not rewritten, full sha.
    pub old: String,
    /// Its subject, so a report can say what went.
    pub subject: String,
    /// Why it was not written. Defaulted on read so a record written before
    /// the field existed still says what it always meant: empty.
    #[serde(default)]
    pub reason: DropReason,
    /// The commit in the base that carries the same change id, full sha.
    /// Set exactly when `reason` is `Superseded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// Why a replay did not write a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Its replayed tree matched its new first parent's.
    #[default]
    Empty,
    /// The base already holds a commit with its change id.
    Superseded,
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
    /// The way back from the session branch: planned once by the resolution
    /// arm of `ff done`, executed by the verb that lands, inside its own
    /// operation.
    pub return_trip: Option<crate::held::Return>,
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
    let Range {
        ordered,
        affected,
        superseded,
    } = range_of(repo, target, tip, change)?;

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
        Change::Message(_) => replay(
            repo,
            &ordered,
            &affected,
            &superseded,
            target,
            change,
            now,
            trees,
        )?,
        Change::Tree { .. } | Change::Onto(_) => {
            let memory = repo.clone().with_object_memory();
            replay(
                &memory,
                &ordered,
                &affected,
                &superseded,
                target,
                change,
                now,
                trees,
            )?;
            replay(
                repo,
                &ordered,
                &affected,
                &superseded,
                target,
                change,
                now,
                trees,
            )?
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
/// oldest-first, which of them the rewrite touches, and which of those it
/// drops unreplayed because the base already holds them. Shared by `plan`
/// and `chain`, which have to agree on all three or their step numbering
/// diverges — and the verb, the replan, and the resolution chain all
/// re-derive the range from the same triple, so a decision made here is
/// made once for every path a rewrite can take.
pub(super) struct Range {
    pub(super) ordered: Vec<gix::ObjectId>,
    pub(super) affected: HashSet<gix::ObjectId>,
    /// Affected commits whose change id the base already carries, each with
    /// the base's commit that carries it. Only ever filled under
    /// [`Change::Onto`]: a reword or a tree change replays onto the same
    /// history the range already stands on.
    pub(super) superseded: HashMap<gix::ObjectId, gix::ObjectId>,
}

pub(super) fn range_of(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
) -> Result<Range> {
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

    let walk = crate::upstream::range(repo, tip, boundary.iter().copied())?;
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

    // 5. Under `Onto`, the affected commits the base already holds by
    // identity. Bounded by the same parents the range walk was, so the base
    // is read down to where the range's floor stands and no further.
    let superseded = match change {
        Change::Onto(onto) => {
            let candidates: Vec<gix::ObjectId> = ordered
                .iter()
                .filter(|&id| affected.contains(id))
                .copied()
                .collect();
            superseded_by(repo, *onto, &boundary, &candidates)?
        }
        Change::Message(_) | Change::Tree { .. } => HashMap::new(),
    };

    Ok(Range {
        ordered,
        affected,
        superseded,
    })
}

/// Which of `candidates` a base already holds by identity: for each candidate
/// carrying a `change-id` header, the newest commit of `onto` above
/// `boundary` carrying the same header. A candidate without the header is
/// never superseded — a derived id is a function of the sha, so it matches
/// no other commit — and when no candidate carries one the base is not
/// walked at all, which keeps a restack of plain-git commits costing what it
/// cost before, and landing on the same sha `git rebase` would.
pub(crate) fn superseded_by(
    repo: &gix::Repository,
    onto: gix::ObjectId,
    boundary: &[gix::ObjectId],
    candidates: &[gix::ObjectId],
) -> Result<HashMap<gix::ObjectId, gix::ObjectId>> {
    let mut wanted: HashMap<crate::changeid::ChangeId, Vec<gix::ObjectId>> = HashMap::new();
    for &id in candidates {
        let obj = repo.find_object(id).map_err(Error::repo)?;
        if let Some(change_id) = crate::changeid::header_of(&obj.data) {
            wanted.entry(change_id).or_default().push(id);
        }
    }
    let mut found: HashMap<gix::ObjectId, gix::ObjectId> = HashMap::new();
    if wanted.is_empty() {
        return Ok(found);
    }
    let walk = crate::upstream::range(repo, onto, boundary.iter().copied())?;
    for info in walk {
        let info = info.map_err(Error::repo)?;
        let obj = repo.find_object(info.id).map_err(Error::repo)?;
        let Some(change_id) = crate::changeid::header_of(&obj.data) else {
            continue;
        };
        // Newest-first, so the first commit of the base carrying the id is
        // the base's current spelling of it.
        if let Some(ids) = wanted.remove(&change_id) {
            for id in ids {
                found.insert(id, info.id);
            }
        }
        if wanted.is_empty() {
            break;
        }
    }
    Ok(found)
}

/// The commits of `target..tip` a replay onto `onto` drops as superseded,
/// each with the base commit that supersedes it — the engine's own answer,
/// for a caller that probes the range before planning it and has to hand
/// the probe the commits the plan will actually replay.
pub(crate) fn superseded_in(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    onto: gix::ObjectId,
) -> Result<HashMap<gix::ObjectId, gix::ObjectId>> {
    Ok(range_of(repo, target, tip, &Change::Onto(onto))?.superseded)
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
#[allow(clippy::too_many_arguments)]
fn replay(
    repo: &gix::Repository,
    ordered: &[gix::ObjectId],
    affected: &HashSet<gix::ObjectId>,
    superseded: &HashMap<gix::ObjectId, gix::ObjectId>,
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
    // `commit.gpgsign` governs every user commit fufu writes, replays
    // included — git needs `rebase.gpgSign` said separately, but in fufu
    // history moves under you, and a restack that quietly unsigned a branch
    // is the failure signing exists to prevent. Resolved once, above the
    // loop; the spawn is per commit, as `git rebase -S` is.
    let signer = crate::sign::resolve(repo, crate::sign::Choice::Config)?;
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
        // A commit the base already holds by identity is not replayed at all:
        // no merge, so a base rewrite that changed its content cannot
        // conflict on a file the branch never touched. `map` points it at
        // its new first parent exactly as an empty drop does — never at the
        // commit that supersedes it, which may sit below the base's tip and
        // would land everything above on the wrong commit.
        if let Some(by) = superseded.get(&id)
            && parents.len() == 1
        {
            map.insert(id, parents[0]);
            dropped.push(Dropped {
                old: id.to_string(),
                subject: subject(repo, id)?,
                reason: DropReason::Superseded,
                by: Some(by.to_string()),
            });
            continue;
        }
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
                reason: DropReason::Empty,
                by: None,
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
        // The inherited signature was stripped above — it is dead the moment
        // the tree or a parent moves — and this is its counterpart. A signer
        // that refuses aborts the whole replay before the ref transaction,
        // so nothing was rewritten.
        let new_id = crate::sign::write_user_commit(repo, signer.as_ref(), commit)?;
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
pub(super) fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
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
pub(super) fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::{DropReason, Dropped};

    #[test]
    fn a_record_without_a_reason_reads_as_empty() {
        let d: Dropped = serde_json::from_str(r#"{"old":"abc","subject":"s"}"#).unwrap();
        assert_eq!(d.reason, DropReason::Empty);
        assert_eq!(d.by, None);
    }

    #[test]
    fn a_superseded_drop_round_trips() {
        let d = Dropped {
            old: "abc".into(),
            subject: "s".into(),
            reason: DropReason::Superseded,
            by: Some("def".into()),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"old":"abc","subject":"s","reason":"superseded","by":"def"}"#
        );
        assert_eq!(serde_json::from_str::<Dropped>(&json).unwrap(), d);
        let empty = Dropped {
            reason: DropReason::Empty,
            by: None,
            ..d
        };
        assert_eq!(
            serde_json::to_string(&empty).unwrap(),
            r#"{"old":"abc","subject":"s","reason":"empty"}"#
        );
    }
}
