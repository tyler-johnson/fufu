//! The evolution log: a manual first-parent walk down a snapshot chain
//! (rev_walk would re-sort; the parent *slot* is the semantics here).
//! Snapshot rows only — commits are `ff log`'s spine, and the open change
//! (`@` row) is [`open_change`]'s to describe.

use gix::prelude::ObjectIdExt;

use crate::error::{Error, Result};
use crate::model::{HeadState, OpenChange, SnapEntry};
use crate::snapshot::chain;

#[derive(Debug, Clone, Default)]
pub struct EvologOptions {
    /// Maximum number of snapshot rows.
    pub limit: Option<usize>,
    /// Chain to walk (branch name or `@detached`); `None` = HEAD's chain.
    pub chain: Option<String>,
    /// Also walk the trash chain (trim's one-deep undo) after the live one.
    pub include_trash: bool,
}

/// The snapshot chain, newest first. A foreign tip (a chain ref hand-pointed
/// at a non-snapshot) terminates the walk silently.
pub fn evolog(repo: &gix::Repository, opts: &EvologOptions) -> Result<Vec<SnapEntry>> {
    let chain_name = match &opts.chain {
        Some(name) => name.clone(),
        None => chain::chain_name(&crate::head::head_state(repo)?),
    };
    let mut rows = walk_chain(
        repo,
        &format!("{}{chain_name}", chain::SNAP_PREFIX),
        opts.limit,
    )?;
    if opts.include_trash {
        rows.extend(walk_chain(
            repo,
            &chain::trash_ref(&chain_name),
            opts.limit,
        )?);
    }
    Ok(rows)
}

/// The open change: HEAD's chain summarized as one row — tip snapshot id,
/// base commit, pending description, and whether the tip tree equals the
/// HEAD tree. Tolerates bare repositories (no workdir → no chain, clean)
/// and unborn branches (no base).
pub fn open_change(repo: &gix::Repository) -> Result<OpenChange> {
    let head = crate::head::head_state(repo)?;
    let branch = chain::chain_name(&head);
    let (base, base_short) = match &head {
        HeadState::Unborn { .. } => (None, None),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => {
            let id = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            let short = id
                .attach(repo)
                .shorten()
                .map(|p| p.to_string())
                .unwrap_or_else(|_| commit.clone());
            (Some(commit.clone()), Some(short))
        }
    };
    let subject = crate::branchmeta::read(repo, &branch)?.pending_description;
    let tip = if repo.workdir().is_some() {
        chain::tip(repo, &format!("{}{branch}", chain::SNAP_PREFIX))?
    } else {
        None
    };
    let (id, time, clean) = match tip {
        None => (None, None, true),
        Some(tip_id) => {
            let commit = repo.find_commit(tip_id).map_err(Error::repo)?;
            let time = commit.time().map_err(Error::repo)?.seconds;
            let tip_tree = commit.tree_id().map_err(Error::repo)?.detach();
            let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
            (Some(tip_id.to_string()), Some(time), tip_tree == head_tree)
        }
    };
    Ok(OpenChange {
        branch,
        id,
        base,
        base_short,
        subject,
        time,
        clean,
    })
}

/// Decode a snapshot commit into its walk shape: `(entry, next)` where
/// `next` is the previous snapshot to continue the walk with.
fn snap_entry(
    repo: &gix::Repository,
    id: gix::ObjectId,
) -> Result<Option<(SnapEntry, Option<gix::ObjectId>)>> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    if obj.kind != gix::objs::Kind::Commit {
        return Ok(None);
    }
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    if !chain::is_snapshot_commit(&commit) {
        return Ok(None);
    }
    let subject = commit.message().summary().to_string();
    let time = commit.committer.time().map_err(Error::repo)?.seconds;
    let parents: Vec<gix::ObjectId> = commit.parents().collect();
    drop(commit);
    drop(obj);

    // Parent slot 1 is the previous snapshot only when it bears the fufu
    // identity; otherwise it IS the base edge (the first snapshot of a chain
    // has HEAD as its only parent).
    let (prev, base) = match parents.as_slice() {
        [] => (None, None),
        [p1, rest @ ..] => {
            if chain::id_is_snapshot(repo, *p1)? {
                (Some(*p1), rest.first().copied())
            } else {
                (None, Some(*p1))
            }
        }
    };
    let short_id = id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string());
    let entry = SnapEntry {
        id: id.to_string(),
        short_id,
        subject,
        time,
        base: base.map(|b| b.to_string()),
        prev: prev.map(|p| p.to_string()),
    };
    Ok(Some((entry, prev)))
}

fn walk_chain(
    repo: &gix::Repository,
    ref_name: &str,
    limit: Option<usize>,
) -> Result<Vec<SnapEntry>> {
    let Some(tip_id) = chain::tip(repo, ref_name)? else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    let mut cur = Some(tip_id);
    while let Some(id) = cur {
        if limit.is_some_and(|n| rows.len() >= n) {
            break;
        }
        let Some((entry, next)) = snap_entry(repo, id)? else {
            break;
        };
        rows.push(entry);
        cur = next;
    }
    Ok(rows)
}
