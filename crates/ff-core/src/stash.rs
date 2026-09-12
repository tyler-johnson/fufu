//! The legacy park, read for the fold. Before the open commit was the park,
//! leaving a branch wrote an ordinary `git stash` entry — byte-shaped like
//! `git stash push -u -m "fufu: wip on <branch>"` — plus
//! `refs/fufu/parked/<branch>` naming the entry by identity. Nothing writes
//! that shape any more: [`crate::park`] folds an entry it meets into an open
//! commit on the first arrival and spends it, and what stays here is the
//! reader that fold needs, the drop that spends the entry the way `git
//! reflog delete --rewrite` would, and the bookkeeping a verb records when
//! it spends one. A verb that only deletes or carries a legacy ref — `ff
//! branch -d`, `ff fold`, a rename, a claim — keeps its ref bookkeeping and
//! leaves the entry on the stash list, the pre-flight behavior for a
//! pre-flight park.

use std::collections::BTreeSet;

use crate::error::{Error, Result};
use crate::ops::record::{OpRecord, RefTransition, RefsTable, StashEffect};
use crate::refs::{self, EditOutcome};

pub const PARKED_PREFIX: &str = "refs/fufu/parked/";
pub const STASH_REF: &str = "refs/stash";

pub fn parked_ref(branch: &str) -> String {
    format!("{PARKED_PREFIX}{branch}")
}

/// One commit object over `tree`, signed as `sig` on both sides: the shape
/// every stash commit took, and the marker commit `ff resolve` mints its
/// session at.
pub(crate) fn write_commit(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    parents: Vec<gix::ObjectId>,
    sig: &gix::actor::Signature,
    message: String,
) -> Result<gix::ObjectId> {
    let commit = gix::objs::Commit {
        tree,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig.clone(),
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };
    Ok(repo.write_object(&commit).map_err(Error::repo)?.detach())
}

/// The decoded anatomy of a stash (WIP) commit.
pub struct StashCommit {
    pub id: gix::ObjectId,
    /// The HEAD the stash was taken on.
    pub base: gix::ObjectId,
    pub base_tree: gix::ObjectId,
    pub wip_tree: gix::ObjectId,
    pub index_tree: gix::ObjectId,
    /// Empty tree when the stash carries no untracked files.
    pub untracked_tree: gix::ObjectId,
}

pub fn read_stash_commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<StashCommit> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit =
        gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()).map_err(Error::repo)?;
    let wip_tree = commit.tree();
    let parents: Vec<gix::ObjectId> = commit.parents().collect();
    drop(commit);
    drop(obj);
    let (&base, rest) = parents
        .split_first()
        .ok_or_else(|| Error::msg(format!("{id} is not a stash commit: no parents")))?;
    let tree_of = |repo: &gix::Repository, id: gix::ObjectId| -> Result<gix::ObjectId> {
        Ok(repo
            .find_commit(id)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach())
    };
    let base_tree = tree_of(repo, base)?;
    let index_tree = match rest.first() {
        Some(&index_commit) => tree_of(repo, index_commit)?,
        None => {
            return Err(Error::msg(format!(
                "{id} is not a stash commit: no index parent"
            )));
        }
    };
    let untracked_tree = match rest.get(1) {
        Some(&untracked_commit) => tree_of(repo, untracked_commit)?,
        None => gix::ObjectId::empty_tree(repo.object_hash()),
    };
    Ok(StashCommit {
        id,
        base,
        base_tree,
        wip_tree,
        index_tree,
        untracked_tree,
    })
}

/// How many files a parked change carries: the tracked diff its wip tree
/// makes against its base, plus the untracked files parked alongside it.
/// The number `ff switch` prints on arrival, asked of the entry itself.
pub fn parked_file_count(repo: &gix::Repository, stash: gix::ObjectId) -> Result<usize> {
    let stash = read_stash_commit(repo, stash)?;
    let mut paths: BTreeSet<String> =
        crate::changestat::tree_diff_stat(repo, stash.base_tree, stash.wip_tree)?
            .files
            .into_iter()
            .map(|file| file.path)
            .collect();
    for (path, _, _) in tree_files(repo, stash.untracked_tree)? {
        paths.insert(path);
    }
    Ok(paths.len())
}

/// Whether `sha` is present in the stash reflog (any line's new value).
pub fn stash_contains(repo: &gix::Repository, sha: gix::ObjectId) -> Result<bool> {
    let Some(reference) = repo.try_find_reference(STASH_REF).map_err(Error::repo)? else {
        return Ok(false);
    };
    let mut platform = reference.log_iter();
    let Some(iter) = platform.rev().map_err(Error::repo)? else {
        return Ok(false);
    };
    for line in iter {
        let line = line.map_err(Error::repo)?;
        if line.new_oid == sha {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The parked entry for a branch, if any.
pub fn parked_entry(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    refs::ref_target(repo, &parked_ref(branch))
}

/// All file entries of a tree, recursively: (path, kind, id).
pub(crate) fn tree_files(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> Result<Vec<(String, gix::objs::tree::EntryKind, gix::ObjectId)>> {
    let mut out = Vec::new();
    fn walk(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        prefix: &str,
        out: &mut Vec<(String, gix::objs::tree::EntryKind, gix::ObjectId)>,
    ) -> Result<()> {
        let obj = repo.find_object(tree).map_err(Error::repo)?.detach();
        for entry in gix::objs::TreeRefIter::from_bytes(&obj.data, repo.object_hash()) {
            let entry = entry.map_err(Error::repo)?;
            let path = if prefix.is_empty() {
                entry.filename.to_string()
            } else {
                format!("{prefix}/{}", entry.filename)
            };
            if entry.mode.is_tree() {
                walk(repo, entry.oid.to_owned(), &path, out)?;
            } else {
                out.push((path, entry.mode.kind(), entry.oid.to_owned()));
            }
        }
        Ok(())
    }
    walk(repo, tree, "", &mut out)?;
    Ok(out)
}

/// Drop a stash entry by identity: replay the stash reflog without its line
/// (chaining rederives, matching `git reflog delete --rewrite`), retargeting
/// or deleting `refs/stash`.
pub fn drop_stash_entry(repo: &gix::Repository, sha: gix::ObjectId) -> Result<()> {
    let Some(reference) = repo.try_find_reference(STASH_REF).map_err(Error::repo)? else {
        return Err(Error::msg("no stash stack exists"));
    };
    let current = reference
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| Error::msg("refs/stash is symbolic"))?;

    // Oldest→newest lines, preserved verbatim.
    struct Line {
        new: gix::ObjectId,
        name: String,
        email: String,
        time_str: String,
        message: String,
    }
    let mut lines: Vec<Line> = Vec::new();
    {
        let mut platform = reference.log_iter();
        let Some(iter) = platform.all().map_err(Error::repo)? else {
            return Err(Error::msg("refs/stash has no reflog"));
        };
        for line in iter {
            let line = line.map_err(Error::repo)?;
            lines.push(Line {
                new: gix::ObjectId::from_hex(line.new_oid).map_err(Error::repo)?,
                name: line.signature.name.to_string(),
                email: line.signature.email.to_string(),
                // The raw `<seconds> <offset>` text, preserved verbatim.
                time_str: line.signature.time.to_string(),
                message: line.message.to_string(),
            });
        }
    }
    // Remove the NEWEST line whose new value is `sha`.
    let position = lines
        .iter()
        .rposition(|line| line.new == sha)
        .ok_or_else(|| Error::msg(format!("{sha} is not in the stash reflog")))?;
    lines.remove(position);

    // Delete, then replay survivors with their original identities and
    // times; each transaction derives the previous value from CAS.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    refs::delete_ref(repo, STASH_REF, current, now)?;
    let mut expected = gix::refs::transaction::PreviousValue::MustNotExist;
    for line in &lines {
        let edit = refs::update_edit(STASH_REF, line.new, expected.clone(), &line.message)?;
        let sig = gix::actor::SignatureRef {
            name: line.name.as_str().into(),
            email: line.email.as_str().into(),
            time: &line.time_str,
        };
        match refs::commit_edits_as(repo, Some(edit), sig)? {
            EditOutcome::Applied => {}
            EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    "refs/stash is contended during replay",
                    vec![],
                ));
            }
        }
        expected = gix::refs::transaction::PreviousValue::MustExistAndMatch(
            gix::refs::Target::Object(line.new),
        );
    }
    Ok(())
}

/// The stash reflog's entries, oldest first: what a verb reads before it
/// plans to spend one, so the planned `refs/stash` can say what is left.
pub(crate) fn lines(repo: &gix::Repository) -> Result<Vec<gix::ObjectId>> {
    Ok(refs::read_ref_log(repo, STASH_REF)?
        .iter()
        .map(|l| l.new)
        .collect())
}

/// Fold the spending of `branch`'s legacy entry into a verb's write-ahead:
/// the parked ref's deletion, the stash effect undo inverts, the pin, and
/// the planned `refs/stash` — the entry left on top of `stash_lines` once
/// this one is out, or no ref at all.
pub(crate) fn spend(
    stash_lines: &mut Vec<gix::ObjectId>,
    planned: &mut RefsTable,
    record: &mut OpRecord,
    pins: &mut Vec<gix::ObjectId>,
    branch: &str,
    sha: gix::ObjectId,
) {
    if let Some(pos) = stash_lines.iter().rposition(|s| *s == sha) {
        stash_lines.remove(pos);
    }
    planned.refs.remove(&parked_ref(branch));
    record.refs.push(RefTransition {
        name: parked_ref(branch),
        old: Some(sha.to_string()),
        new: None,
    });
    record.stash.push(StashEffect::Drop {
        branch: branch.to_string(),
        stash: sha.to_string(),
    });
    pins.push(sha);
    match stash_lines.last() {
        Some(tip) => {
            planned.refs.insert(STASH_REF.to_string(), tip.to_string());
        }
        None => {
            planned.refs.remove(STASH_REF);
        }
    }
}
