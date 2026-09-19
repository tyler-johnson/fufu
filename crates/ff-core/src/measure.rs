//! What a commit's own change is measured against.
//!
//! One parent or none is no question: the parent's tree, or the empty tree.
//! A merge is measured against the auto-merge of its parents, jj's rule, so
//! a clean merge did nothing of its own and a merge that resolved a conflict
//! or carried an edit shows exactly that. When the auto-merge cannot be
//! made, because the parents share no ancestor or because it conflicts, the
//! first parent stands in and the reason travels with the tree, so the verb
//! can say so rather than print a diff nobody asked for.
//!
//! `ff show`, `ff log -p`, and `ff diff -r` spend this one measure, and
//! replay reads it to carry a merge through a rewrite, so the four cannot
//! disagree about what a merge did.

use crate::error::{Error, Result};
use crate::futures;

/// What a commit's own change is measured against, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Against {
    /// Its one parent's tree, or the empty tree for a root commit.
    Parent,
    /// The auto-merge of its parents: what the merge itself did.
    AutoMerge,
    /// Its first parent's tree, because the auto-merge could not be made.
    FirstParent(Fallback),
}

/// Why a merge fell back to its first parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// Two parents with no common ancestor.
    NoBase,
    /// The auto-merge conflicts in these paths, sorted.
    Conflicts(Vec<String>),
}

/// The tree a commit is measured against, with the reason and the commit
/// that tree belongs to when it is a commit's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measure {
    /// The tree the commit's own tree is diffed against. Readable through
    /// the `repo` handed to [`measure`]; under a memory handle an auto-merge
    /// tree exists nowhere else.
    pub tree: gix::ObjectId,
    pub against: Against,
    /// The commit whose tree `tree` is, when it is a commit's: the only
    /// parent, or the first parent under the fallback. `None` for a root
    /// commit and for an auto-merge.
    pub commit: Option<gix::ObjectId>,
}

impl Against {
    /// The word JSON carries: `parent`, `auto-merge`, `first-parent`.
    pub fn word(&self) -> &'static str {
        match self {
            Against::Parent => "parent",
            Against::AutoMerge => "auto-merge",
            Against::FirstParent(_) => "first-parent",
        }
    }
}

/// The tree a commit's own change is measured against. One parent or none:
/// that parent's tree or the empty tree. A merge: `merge_trees(base, p1, p2)`
/// with `base = merge_base(p1, p2)`, folded left to right over further
/// parents with `base_i = merge_base(p1, p_i)` and the accumulated tree as
/// ours. No base, or a fold that conflicts, is the first parent's tree with
/// the reason. Objects the merge writes go through `repo`: pass
/// `repo.clone().with_object_memory()` to read without leaving an auto-merge
/// tree in the store, which is what the read verbs do; replay passes the
/// real handle when it wants the tree kept.
pub fn measure(repo: &gix::Repository, commit: gix::ObjectId) -> Result<Measure> {
    let parents: Vec<gix::ObjectId> = repo
        .find_commit(commit)
        .map_err(Error::repo)?
        .parent_ids()
        .map(|p| p.detach())
        .collect();
    let Some(&first) = parents.first() else {
        return Ok(Measure {
            tree: gix::ObjectId::empty_tree(repo.object_hash()),
            against: Against::Parent,
            commit: None,
        });
    };
    let first_tree = futures::tree_of(repo, first)?;
    if parents.len() == 1 {
        return Ok(Measure {
            tree: first_tree,
            against: Against::Parent,
            commit: Some(first),
        });
    }

    let fallback = |why: Fallback| Measure {
        tree: first_tree,
        against: Against::FirstParent(why),
        commit: Some(first),
    };
    let mut ours = first_tree;
    for &theirs in &parents[1..] {
        let base = match repo.merge_base(first, theirs) {
            Ok(base) => base.detach(),
            Err(gix::repository::merge_base::Error::NotFound { .. }) => {
                return Ok(fallback(Fallback::NoBase));
            }
            Err(e) => return Err(Error::repo(e)),
        };
        let base_tree = futures::tree_of(repo, base)?;
        let theirs_tree = futures::tree_of(repo, theirs)?;
        let options = repo.tree_merge_options().map_err(Error::repo)?;
        let mut outcome = repo
            .merge_trees(base_tree, ours, theirs_tree, Default::default(), options)
            .map_err(Error::repo)?;
        let conflicts = futures::unresolved(&outcome);
        if !conflicts.is_empty() {
            return Ok(fallback(Fallback::Conflicts(conflicts)));
        }
        ours = outcome.tree.write().map_err(Error::repo)?.detach();
    }
    Ok(Measure {
        tree: ours,
        against: Against::AutoMerge,
        commit: None,
    })
}
