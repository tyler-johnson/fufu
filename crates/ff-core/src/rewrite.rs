//! The writing half of the engine [`crate::futures::probe`] simulates: it
//! re-parents a range of commits and writes the rewritten objects, moving no
//! refs of its own. Every rewrite verb — reword today, absorb and lift later
//! — aims here rather than forking its own commit-writing logic.

use std::collections::{HashMap, HashSet};

use gix::bstr::BString;
use gix::prelude::ObjectIdExt;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ops::record::RefTransition;

/// One commit's old→new identity, full shas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewrite {
    pub old: String,
    pub new: String,
}

/// What changes about the named commit. Absorb and lift add their variants
/// here rather than forking the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Message(String),
}

/// The result of re-parenting `target..tip` after applying a [`Change`] to
/// `target`.
#[derive(Debug, Clone)]
pub struct RewritePlan {
    /// Every rewritten commit, oldest-first, target first.
    pub rewrites: Vec<Rewrite>,
    /// Local heads sitting inside the rewritten range, sorted by ref name.
    pub carried: Vec<RefTransition>,
    pub new_tip: gix::ObjectId,
}

/// Apply `change` to `target` and re-parent every commit between it and
/// `tip`. Writes commit objects; moves no refs.
pub fn plan(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
    now: i64,
) -> Result<RewritePlan> {
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
        let target_short = short(repo, target);
        let tip_short = short(repo, tip);
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

    // 5. Rewrite each affected commit in order.
    let committer = crate::refs::user_signature(repo, now)?;
    let mut map: HashMap<gix::ObjectId, gix::ObjectId> = HashMap::new();
    let mut rewrites: Vec<Rewrite> = Vec::new();
    for &id in &ordered {
        if !affected.contains(&id) {
            continue;
        }
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;

        let tree = gix::ObjectId::from_hex(commit_ref.tree).map_err(Error::repo)?;
        let parents: Vec<gix::ObjectId> = commit_ref
            .parents
            .iter()
            .map(|hex| {
                let old = gix::ObjectId::from_hex(hex).map_err(Error::repo)?;
                Ok(map.get(&old).copied().unwrap_or(old))
            })
            .collect::<Result<_>>()?;
        let author = commit_ref.author.to_owned().map_err(Error::repo)?;
        let message: BString = if id == target {
            match change {
                Change::Message(text) => crate::close::normalize_message(text).into(),
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
        carried,
        new_tip,
    })
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

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}
