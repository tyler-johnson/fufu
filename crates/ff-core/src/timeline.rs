//! The interleaved timeline: a manual first-parent walk down a snapshot
//! chain (rev_walk would re-sort; the parent *slot* is the semantics here),
//! interleaved with base rows wherever the base edge moved — HEAD-move events
//! for free — and terminated by the anchor commit the chain grew from.

use gix::prelude::ObjectIdExt;

use crate::error::{Error, Result};
use crate::model::{SnapEntry, TimelineRow};
use crate::snapshot::chain;

#[derive(Debug, Clone, Default)]
pub struct TimelineOptions {
    /// Maximum number of *snapshot* rows; base rows ride free.
    pub limit: Option<usize>,
    /// Chain to walk (branch name or `@detached`); `None` = HEAD's chain.
    pub chain: Option<String>,
    /// Also walk the trash chain (trim's one-deep undo) after the live one.
    pub include_trash: bool,
}

pub fn timeline(repo: &gix::Repository, opts: &TimelineOptions) -> Result<Vec<TimelineRow>> {
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

/// Decode a snapshot commit into its timeline shape: `(entry, next)` where
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
) -> Result<Vec<TimelineRow>> {
    let Some(tip) = repo.try_find_reference(ref_name).map_err(Error::repo)? else {
        return Ok(Vec::new());
    };
    let Some(tip_id) = tip.target().try_id().map(|id| id.to_owned()) else {
        return Ok(Vec::new());
    };

    let mut rows = Vec::new();
    let mut cur = Some(tip_id);
    // The base edge of the row emitted last (newest-first walk): when it
    // differs from the next snapshot's base, HEAD moved between the two —
    // emit the newer base as an event row.
    let mut last_base: Option<String> = None;
    let mut snap_rows = 0usize;

    while let Some(id) = cur {
        if limit.is_some_and(|n| snap_rows >= n) {
            // Cut by the limit: no anchor, the chain continues below the fold.
            return Ok(rows);
        }
        let Some((entry, next)) = snap_entry(repo, id)? else {
            // Not a snapshot: only reachable if a chain ref was hand-pointed
            // at a foreign commit — treat it as the anchor.
            rows.push(TimelineRow::Base(crate::log::entry_for(repo, id)?));
            return Ok(rows);
        };
        if !rows.is_empty()
            && last_base != entry.base
            && let Some(moved) = &last_base
        {
            let base_id = gix::ObjectId::from_hex(moved.as_bytes()).map_err(Error::repo)?;
            rows.push(TimelineRow::Base(crate::log::entry_for(repo, base_id)?));
        }
        last_base = entry.base.clone();
        rows.push(TimelineRow::Snapshot(entry));
        snap_rows += 1;
        cur = next;
    }

    // Natural end of the chain: the oldest snapshot's base is the anchor.
    if let Some(anchor) = last_base {
        let base_id = gix::ObjectId::from_hex(anchor.as_bytes()).map_err(Error::repo)?;
        rows.push(TimelineRow::Base(crate::log::entry_for(repo, base_id)?));
    }
    Ok(rows)
}
