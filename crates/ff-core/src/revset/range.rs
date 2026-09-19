//! A set read as one range: what `ff diff -r` measures.
//!
//! jj's rule for `jj diff -r`: a set's patch runs from its root's first
//! parent to its head, so the set has to be connected with exactly one of
//! each. Connectivity is read off the members and their parent links in one
//! pass — no second walk, since the members are already in hand and the
//! parent edges are the only edges git stores.
//!
//! The open change is the one member without a commit of its own. The
//! evaluator does not hang it below HEAD, so `heads(HEAD~2..@)` is two rows,
//! and this module puts it back: `@` is HEAD's child for the purpose of
//! counting heads and roots.

use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};

use super::{Rev, Revset, resolve};

/// A connected set with one head and one root: what `ff diff -r` measures,
/// from the root's first parent to the head, jj's rule for `jj diff -r`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub root: Rev,
    pub head: Rev,
}

impl Revset {
    /// The set as one range, or `usage/revset-not-a-range`.
    pub fn range(&self, repo: &gix::Repository) -> Result<Range> {
        let members = self.members(repo)?;
        let has_open = members.iter().any(|m| m.is_open());
        let ids: HashSet<gix::ObjectId> = members
            .iter()
            .filter_map(|m| match m {
                Rev::Commit(id) => Some(id.object_id()),
                Rev::Open(_) => None,
            })
            .collect();
        let mut parents: HashMap<gix::ObjectId, Vec<gix::ObjectId>> = HashMap::new();
        for id in &ids {
            let commit = repo.find_commit(*id).map_err(Error::repo)?;
            parents.insert(*id, commit.parent_ids().map(|p| p.detach()).collect());
        }

        // The open change sits on HEAD. A set that has it without HEAD is in
        // two pieces however the commits below connect.
        let head_commit = if has_open {
            resolve::open_commit(repo)?
        } else {
            None
        };
        if has_open && !ids.is_empty() && !head_commit.is_some_and(|h| ids.contains(&h)) {
            return Err(self.not_a_range(format!(
                "`{}` includes the open change but not HEAD, the commit it sits on, so it is not one range",
                self.src
            )));
        }

        let mut has_child: HashSet<gix::ObjectId> = HashSet::new();
        for links in parents.values() {
            for parent in links {
                if ids.contains(parent) {
                    has_child.insert(*parent);
                }
            }
        }
        if let Some(h) = head_commit {
            has_child.insert(h);
        }

        let mut heads: Vec<Rev> = Vec::new();
        let mut roots: Vec<Rev> = Vec::new();
        if has_open {
            heads.push(Rev::Open(None));
            if ids.is_empty() {
                roots.push(Rev::Open(None));
            }
        }
        for member in &members {
            let Rev::Commit(id) = member else { continue };
            let oid = id.object_id();
            if !has_child.contains(&oid) {
                heads.push(*member);
            }
            if !parents[&oid].iter().any(|p| ids.contains(p)) {
                roots.push(*member);
            }
        }

        if heads.len() != 1 {
            return Err(self.not_a_range(format!(
                "`{}` has {} heads, and a set's patch runs from one root's parent to one head",
                self.src,
                heads.len()
            )));
        }
        if roots.len() != 1 {
            return Err(self.not_a_range(format!(
                "`{}` has {} roots, a gap between members or a fork below them, and a set's patch runs from one root's parent to one head",
                self.src,
                roots.len()
            )));
        }
        let (root, head) = (roots[0], heads[0]);
        if let Rev::Commit(id) = root
            && parents[&id.object_id()].len() > 1
        {
            let root_short = crate::sha::short_oid(id.object_id());
            let head_spelled = match head {
                Rev::Open(_) => "@".to_string(),
                Rev::Commit(h) => crate::sha::short_oid(h.object_id()),
            };
            return Err(Error::coded(
                "usage/revset-not-a-range",
                format!(
                    "`{}` starts at {root_short}, a merge, and which parent to measure from is a choice",
                    self.src
                ),
                vec![
                    format!("ff diff --from {root_short}^ --to {head_spelled}"),
                    format!("ff diff --from {root_short}^2 --to {head_spelled}"),
                ],
            ));
        }
        Ok(Range { root, head })
    }

    fn not_a_range(&self, message: String) -> Error {
        Error::coded(
            "usage/revset-not-a-range",
            message,
            vec![
                format!("ff log -r \"{}\"", self.src),
                "ff diff --from <rev> --to <rev>".into(),
            ],
        )
    }
}
