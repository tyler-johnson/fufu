//! The machine record: `op.json` plus the ref table and index tree that ride
//! with it, in a parentless commit hanging off the operation.
//!
//! A record exists only where there is something to record. A capture
//! changes no ref by invariant, so it has no ref transitions, no index
//! transition, and no verb line — and on the highest-volume path in the tool
//! (1584 captures against 64 verb ops, measured) an unconditional record
//! would be an extra commit, tree and blob apiece. That is the difference
//! between merging the two logs and doubling the store, which is why the
//! capture invariant is load-bearing here rather than decorative.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::ForeignChange;
use crate::refs;

const RECORD_VERSION: u32 = 1;

/// One ref's transition, full shas (or `None` for created/deleted ends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefTransition {
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A stash-stack effect performed by the op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StashEffect {
    Push { branch: String, stash: String },
    Drop { branch: String, stash: String },
}

/// A worktree an operation made or took away.
///
/// This is the only effect fufu records whose inverse touches the filesystem
/// outside `.git`: every other undo restores files fufu already holds, and
/// this one must create a checkout or delete one. The removal arm carries a
/// capture id precisely so the inverse has something to re-materialize from —
/// the tree stood on the removed worktree's own chain, and the id is the door
/// into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeEffect {
    Add {
        id: String,
        path: String,
        branch: String,
    },
    Remove {
        id: String,
        /// The checkout path, empty when the entry no longer named one — a
        /// stale entry whose checkout is already gone is still removed, and
        /// the effect must say so without a path to name.
        path: String,
        branch: Option<String>,
        /// The capture taken before the tree was torn down, on the removed
        /// tree's own chain. `None` only when that tree had nothing to
        /// capture and no operations of its own.
        capture: Option<String>,
    },
}

/// A pending-description change (old/new text, `None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionTransition {
    pub branch: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A recorded-parent change (old/new branch names, `None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentTransition {
    pub branch: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// An editing session opened or ended on a branch (`None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTransition {
    pub branch: String,
    pub old: Option<crate::branchmeta::Session>,
    pub new: Option<crate::branchmeta::Session>,
}

/// A held rewrite recorded or cleared on a branch (`None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldTransition {
    pub branch: String,
    pub old: Option<crate::held::Held>,
    pub new: Option<crate::held::Held>,
}

/// One push this repository made, recorded after the fact.
///
/// Publish is the one verb whose effect is not a local ref, so there is
/// nothing to diff a write-ahead claim against — the remote is not
/// observable without the network. What the row is for is the question
/// sync and status keep getting wrong on their own: *is the tracking tip a
/// tip this branch published?* Storing `to` rather than the set of commits
/// answers it in O(1) and is sound by construction — if the remote stands
/// exactly where you last sent it, everything reachable from it that you
/// now lack was yours when you sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Published {
    pub remote: String,
    pub remote_branch: String,
    /// The lease: the tracking tip as last seen. `None` is git's "must not
    /// exist", the spelling that creates or re-creates a shared copy.
    pub from: Option<String>,
    /// What the remote holds now.
    pub to: String,
}

/// A resolution session opened or ended on a branch (`None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveTransition {
    pub branch: String,
    pub old: Option<crate::held::Resolve>,
    pub new: Option<crate::held::Resolve>,
}

/// The machine record of one operation (`op.json`).
///
/// `pre_snapshot` and `index_tree` are gone from the old journal shape
/// because both became structure: the op's own tree *is* the worktree it
/// carries, so there is no separate snapshot to point at, and the index tree
/// is a subtree of this record rather than a sha inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpRecord {
    pub version: u32,
    /// The fufu verb (`commit`, `switch`, …) or `reconcile`/`init`.
    pub verb: String,
    /// Human summary, doubling as the commit subject.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    pub time: i64,
    /// Branch (chain name) the op ran on, if on one. Mirrors `fufu-branch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// HEAD's commit when the op ran. Mirrors `fufu-base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The previous operation in the log. Mirrors `fufu-prev`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    /// The previous operation on this op's branch. Mirrors
    /// `fufu-prev-branch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_on_branch: Option<String>,
    /// The newest op of the previous segment. Mirrors `fufu-prev-segment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_segment: Option<String>,
    /// Worktree files dropped for exceeding `fufu.maxFileSize`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// HEAD's transition: `ref:<name>` for symbolic, sha for detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<RefTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stash: Vec<StashEffect>,
    /// A worktree the op made or took away. Optional and skipped when absent,
    /// so no `RECORD_VERSION` bump: a record written before it existed still
    /// reads, and one written now still reads to an older binary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktree: Vec<WorktreeEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<DescriptionTransition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentTransition>,
    /// An editing session opened or ended. Spelled `edit_session` to stay
    /// clear of `VerbOp.session`, which is the provenance tag and an
    /// unrelated thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_session: Option<SessionTransition>,
    /// A rewrite held or released. Recorded like a session, and for the same
    /// reason: undo has to be able to put it back. No `RECORD_VERSION` bump
    /// for this or `resolving`: both are optional and skipped when absent, so
    /// a record written before they existed still reads, and one written now
    /// still reads to an older binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<HeldTransition>,
    /// A resolution session opened or ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolving: Option<ResolveTransition>,
    /// The rewrite map: old→new for every commit this op rewrote. The log is
    /// already the authority and already pins the old commits, so undo and
    /// `ff trim` cover the map for free.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrites: Vec<crate::rewrite::Rewrite>,
    /// Commits a replay removed rather than rewrote — their tree matched
    /// the new first parent's, so nothing was written for them. Kept for
    /// the same reason the rewrite map is, so a later reader can tell
    /// fufu's own removals apart from commits it has never seen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<crate::rewrite::Dropped>,
    /// The operation this one undoes, when the verb is `undo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<String>,
    /// The push this operation made, when the verb is `publish`. Optional
    /// and skipped when absent, so no `RECORD_VERSION` bump: a record
    /// written before it existed still reads, and one written now still
    /// reads to an older binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<Published>,
    /// How many ops back the undo stack currently stands. `ff undo` repeats
    /// and `ff redo` walks back up, so the cursor is state the log carries
    /// rather than a file beside it — one authority, one trim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_cursor: Option<u32>,
}

impl OpRecord {
    pub fn new(verb: impl Into<String>, summary: impl Into<String>, time: i64) -> Self {
        OpRecord {
            version: RECORD_VERSION,
            verb: verb.into(),
            summary: summary.into(),
            argv: Vec::new(),
            time,
            branch: None,
            base: None,
            prev: None,
            prev_on_branch: None,
            prev_segment: None,
            skipped: Vec::new(),
            head: None,
            refs: Vec::new(),
            stash: Vec::new(),
            worktree: Vec::new(),
            description: None,
            parent: None,
            edit_session: None,
            held: None,
            resolving: None,
            rewrites: Vec::new(),
            dropped: Vec::new(),
            undo_of: None,
            undo_cursor: None,
            published: None,
        }
    }
}

/// The full last-seen ref table: HEAD plus every tracked ref.
/// Remotes are deliberately excluded (their churn stays silent).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefsTable {
    /// `ref:<full-name>` while on a branch (born or not), sha when detached.
    pub head: String,
    /// Full ref name → sha, sorted by name.
    pub refs: BTreeMap<String, String>,
}

const TRACKED_PREFIXES: [&str; 3] = ["refs/heads/", "refs/tags/", "refs/fufu/parked/"];

/// Read the current tracked-ref state. Symbolic branch refs (rare) and tag
/// refs are recorded by their direct target.
///
/// Excludes branch-keyed state this worktree does not own: refs under a
/// branch held by another worktree (the branch itself and its parked state)
/// are somebody else's and an undo here must not be expected to move them.
/// Tags and `refs/stash` stay — one stack for the whole repository.
///
/// Deliberately off the capture path: a capture inherits its predecessor's
/// table instead of observing one. See [`super::append::commit_op`].
pub fn observe_refs(repo: &gix::Repository) -> Result<RefsTable> {
    let mut table = RefsTable::default();
    let head = repo.head().map_err(Error::repo)?;
    table.head = match head.kind {
        gix::head::Kind::Unborn(name) => format!("ref:{}", name.as_bstr()),
        gix::head::Kind::Detached { target, peeled } => peeled.unwrap_or(target).to_string(),
        gix::head::Kind::Symbolic(reference) => format!("ref:{}", reference.name.as_bstr()),
    };
    let held = crate::linked::held_branches(repo)?;
    let platform = repo.references().map_err(Error::repo)?;
    for prefix in TRACKED_PREFIXES {
        let iter = platform.prefixed(prefix).map_err(Error::repo)?;
        for reference in iter {
            let reference = reference.map_err(|err| {
                Error::coded(
                    "op/unreadable",
                    format!("ref iteration failed: {err}"),
                    vec![],
                )
            })?;
            let name = reference.name().as_bstr().to_string();
            if crate::linked::owned_elsewhere(&name, &held) {
                continue;
            }
            if let Some(id) = reference.target().try_id() {
                table.refs.insert(name, id.to_string());
            }
        }
    }
    if let Some(stash) = refs::ref_target(repo, "refs/stash")? {
        table.refs.insert("refs/stash".into(), stash.to_string());
    }
    Ok(table)
}

impl RefsTable {
    /// Serialize as the `refs` blob: `HEAD` line first, then `<sha> <name>`
    /// sorted by name — packed-refs-shaped, one state per line.
    pub fn to_blob(&self) -> String {
        let mut out = format!("{} HEAD\n", self.head);
        for (name, sha) in &self.refs {
            out.push_str(&format!("{sha} {name}\n"));
        }
        out
    }

    pub fn from_blob(text: &str) -> Result<Self> {
        let mut table = RefsTable::default();
        for line in text.lines() {
            let Some((value, name)) = line.split_once(' ') else {
                return Err(Error::coded(
                    "op/unreadable",
                    format!("malformed refs line: {line:?}"),
                    vec![],
                ));
            };
            if name == "HEAD" {
                table.head = value.to_string();
            } else {
                table.refs.insert(name.to_string(), value.to_string());
            }
        }
        if table.head.is_empty() {
            return Err(Error::coded(
                "op/unreadable",
                "refs table has no HEAD line",
                vec![],
            ));
        }
        Ok(table)
    }

    /// The differences carrying `self` (expected) to `current` (observed).
    pub fn diff(&self, current: &RefsTable) -> Vec<ForeignChange> {
        let mut out = Vec::new();
        if self.head != current.head {
            out.push(ForeignChange {
                name: "HEAD".into(),
                old: Some(self.head.clone()),
                new: Some(current.head.clone()),
                hint: None,
            });
        }
        let names: std::collections::BTreeSet<&String> =
            self.refs.keys().chain(current.refs.keys()).collect();
        for name in names {
            let old = self.refs.get(name);
            let new = current.refs.get(name);
            if old != new {
                out.push(ForeignChange {
                    name: name.to_string(),
                    old: old.cloned(),
                    new: new.cloned(),
                    hint: None,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(head: &str, pairs: &[(&str, &str)]) -> RefsTable {
        RefsTable {
            head: head.into(),
            refs: pairs
                .iter()
                .map(|(n, s)| ((*n).to_string(), (*s).to_string()))
                .collect(),
        }
    }

    #[test]
    fn blob_round_trips() {
        let t = table(
            "ref:refs/heads/main",
            &[("refs/heads/main", &"a".repeat(40))],
        );
        assert_eq!(RefsTable::from_blob(&t.to_blob()).unwrap(), t);
    }

    #[test]
    fn diff_names_head_and_every_moved_ref() {
        let before = table(
            "ref:refs/heads/main",
            &[("refs/heads/main", &"a".repeat(40))],
        );
        let after = table(
            "ref:refs/heads/feat",
            &[("refs/heads/feat", &"b".repeat(40))],
        );
        let changes = before.diff(&after);
        let names: Vec<&str> = changes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["HEAD", "refs/heads/feat", "refs/heads/main"]);
    }

    #[test]
    fn a_headless_blob_is_refused() {
        let err =
            RefsTable::from_blob(&format!("{} refs/heads/main\n", "a".repeat(40))).unwrap_err();
        assert_eq!(err.id(), "op/unreadable");
    }
}
