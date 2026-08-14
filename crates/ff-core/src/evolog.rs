//! The evolution log: a manual first-parent walk down a snapshot chain
//! (rev_walk would re-sort; the parent *slot* is the semantics here).
//! Snapshot rows only — commits are `ff log`'s spine, and the open change
//! (`@` row) is [`open_change`]'s to describe.

use gix::prelude::ObjectIdExt;
use std::collections::HashMap;

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
    let mut rows: Vec<_> = walk_chain_trees(
        repo,
        &format!("{}{chain_name}", chain::SNAP_PREFIX),
        opts.limit,
    )?
    .into_iter()
    .map(|(entry, _)| entry)
    .collect();
    if opts.include_trash {
        rows.extend(
            walk_chain_trees(repo, &chain::trash_ref(&chain_name), opts.limit)?
                .into_iter()
                .map(|(entry, _)| entry),
        );
    }
    fill_short_ids(repo, &mut rows);
    Ok(rows)
}

/// Every snapshot id on one chain ref, in walk order (newest first), no limit.
/// This is a resolution domain in its materialized form — ids only, no
/// abbreviation, which is what makes it affordable over a whole chain.
pub fn ref_ids(repo: &gix::Repository, ref_name: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    walk_chain_while(repo, ref_name, &mut |entry: SnapEntry, _tree| {
        ids.push(entry.id);
        true
    })?;
    Ok(ids)
}

/// Abbreviate the ids of rows that are about to be shown or serialized.
/// `shorten()` is an object-store prefix lookup, not a string operation —
/// affordable per displayed row, ruinous per chain link — so the walk leaves
/// `short_id` empty and only this fills it.
fn fill_short_ids(repo: &gix::Repository, rows: &mut [SnapEntry]) {
    for row in rows {
        row.short_id = gix::ObjectId::from_hex(row.id.as_bytes())
            .ok()
            .and_then(|id| id.attach(repo).shorten().ok())
            .map(|prefix| prefix.to_string())
            .unwrap_or_else(|| row.id.clone());
    }
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
    let (id, time, clean, tip_tree) = match tip {
        None => (None, None, true, None),
        Some(tip_id) => {
            let commit = repo.find_commit(tip_id).map_err(Error::repo)?;
            let time = commit.time().map_err(Error::repo)?.seconds;
            let tip_tree = commit.tree_id().map_err(Error::repo)?.detach();
            let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
            (
                Some(tip_id.to_string()),
                Some(time),
                tip_tree == head_tree,
                Some(tip_tree),
            )
        }
    };

    // Compute pending hash — any failure → None, open_change must not gain new failure modes.
    let pending = (|| {
        repo.workdir()?;
        if clean && subject.is_none() {
            return None;
        }
        if !clean {
            // Dirty — tip exists. tree = tip's tree, timestamp = tip's time.
            let msg = crate::close::normalize_message(subject.as_deref().unwrap_or(""));
            return pending_commit_hash(
                repo,
                tip_tree?,
                base.as_deref()
                    .and_then(|b| gix::ObjectId::from_hex(b.as_bytes()).ok()),
                &msg,
                time?,
            );
        }
        // Clean + subject.is_some() — pending empty commit.
        let head_tree = repo.head_tree_id_or_empty().ok()?.detach();
        if let Some(tip_time) = time {
            // tip exists.
            let msg = crate::close::normalize_message(subject.as_deref().unwrap_or(""));
            Some(pending_commit_hash(
                repo,
                head_tree,
                base.as_deref()
                    .and_then(|b| gix::ObjectId::from_hex(b.as_bytes()).ok()),
                &msg,
                tip_time,
            )?)
        } else if let Some(base_hex) = &base {
            // no tip, HEAD born — use HEAD commit time.
            let head_commit_id = gix::ObjectId::from_hex(base_hex.as_bytes()).ok()?;
            let head_commit = repo.find_commit(head_commit_id).ok()?;
            let head_time = head_commit.time().ok()?.seconds;
            let msg = crate::close::normalize_message(subject.as_deref().unwrap_or(""));
            Some(pending_commit_hash(
                repo,
                head_tree,
                Some(head_commit_id),
                &msg,
                head_time,
            )?)
        } else {
            // no tip, unborn — no timestamp source.
            None
        }
    })();

    Ok(OpenChange {
        branch,
        id,
        base,
        base_short,
        subject,
        time,
        clean,
        pending,
    })
}

/// The hash the close would mint: build the commit object and hash it
/// WITHOUT writing it. Any failure (no identity, serialization) → `None` —
/// a log view must never fail over a pending id.
fn pending_commit_hash(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    parent: Option<gix::ObjectId>,
    message: &str,
    when: i64,
) -> Option<String> {
    use gix::objs::WriteTo as _;
    let sig = crate::refs::user_signature(repo, when).ok()?;
    let commit = gix::objs::Commit {
        tree,
        parents: parent.into_iter().collect::<Vec<_>>().into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };
    let mut buf = Vec::new();
    commit.write_to(&mut buf).ok()?;
    gix::objs::compute_hash(repo.object_hash(), gix::objs::Kind::Commit, &buf)
        .ok()
        .map(|id| id.to_string())
}

/// Decode a snapshot commit into its walk shape: `(entry, next, tree)` where
/// `next` is the previous snapshot to continue the walk with and `tree` is
/// the snapshot commit's tree id.
fn snap_entry(
    repo: &gix::Repository,
    id: gix::ObjectId,
) -> Result<Option<(SnapEntry, Option<gix::ObjectId>, gix::ObjectId)>> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    if obj.kind != gix::objs::Kind::Commit {
        return Ok(None);
    }
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    if !chain::is_snapshot_commit(&commit) {
        return Ok(None);
    }
    let tree = commit.tree();
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
    let entry = SnapEntry {
        id: id.to_string(),
        // Filled by `fill_short_ids` on the display paths only.
        short_id: String::new(),
        subject,
        time,
        base: base.map(|b| b.to_string()),
        prev: prev.map(|p| p.to_string()),
    };
    Ok(Some((entry, prev, tree)))
}

/// Walk a chain newest-first, handing each snapshot and its tree to `visit`.
/// The walk ends when `visit` returns false, when the chain runs out, or when
/// a foreign commit terminates it. Every chain walk goes through here, so
/// "how far do we walk" is one decision per caller rather than a habit.
fn walk_chain_while(
    repo: &gix::Repository,
    ref_name: &str,
    visit: &mut dyn FnMut(SnapEntry, gix::ObjectId) -> bool,
) -> Result<()> {
    let Some(tip_id) = chain::tip(repo, ref_name)? else {
        return Ok(());
    };
    let mut cur = Some(tip_id);
    while let Some(id) = cur {
        let Some((entry, next, tree)) = snap_entry(repo, id)? else {
            break;
        };
        if !visit(entry, tree) {
            break;
        }
        cur = next;
    }
    Ok(())
}

fn walk_chain_trees(
    repo: &gix::Repository,
    ref_name: &str,
    limit: Option<usize>,
) -> Result<Vec<(SnapEntry, gix::ObjectId)>> {
    let mut rows = Vec::new();
    let mut collect = |entry, tree| {
        rows.push((entry, tree));
        !limit.is_some_and(|n| rows.len() >= n)
    };
    walk_chain_while(repo, ref_name, &mut collect)?;
    Ok(rows)
}

/// For each displayed commit id (full hex), the newest live-chain snapshot
/// whose base is the commit's first parent and whose tree equals the commit's
/// tree — the evolog drill-in anchor. Root commits match base-less snapshots.
/// Commits with no match are absent from the map. Trash chains never contribute.
pub fn segment_anchors(
    repo: &gix::Repository,
    commit_ids: &[String],
) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();
    if commit_ids.is_empty() {
        return Ok(result);
    }
    let head = crate::head::head_state(repo)?;
    let chain_name = chain::chain_name(&head);
    let ref_name = format!("{}{chain_name}", chain::SNAP_PREFIX);

    // What the displayed commits are asking, keyed the way a snapshot answers:
    // (base, tree). Two commits can share a key — an empty commit beside the
    // one whose tree it repeats — so a key answers a list.
    let mut wanted: HashMap<(Option<String>, gix::ObjectId), Vec<&str>> = HashMap::new();
    // A snapshot cannot be based on a commit that did not exist when it was
    // taken, so nothing below the oldest base's time can anchor anything here
    // and the walk stops there. A base we cannot read (a root commit, a
    // shallow boundary) drops the floor away rather than risk a lost anchor.
    // Rewrites are safe — a rewritten base is a different object, so its
    // snapshots are younger too. Only a committer date faked into the future
    // can cut the walk short, and it costs that row its letters, nothing more.
    let mut floor: Option<i64> = Some(i64::MAX);
    for id in commit_ids {
        let oid = gix::ObjectId::from_hex(id.as_bytes()).map_err(Error::repo)?;
        let commit = repo.find_commit(oid).map_err(Error::repo)?;
        let parent = commit.parent_ids().next().map(|p| p.detach());
        let tree = commit.tree_id().map_err(Error::repo)?.detach();
        let base_time = parent
            .and_then(|p| repo.find_commit(p).ok())
            .and_then(|base| base.time().ok())
            .map(|time| time.seconds);
        floor = match base_time {
            Some(seconds) => floor.map(|f| f.min(seconds)),
            None => None,
        };
        wanted
            .entry((parent.map(|p| p.to_string()), tree))
            .or_default()
            .push(id.as_str());
    }
    let floor = floor.unwrap_or(i64::MIN);

    // Newest first, so the first snapshot to answer a key is the newest one —
    // the same pick the full-chain sweep made, reached without walking past it.
    let mut anchor = |entry: SnapEntry, tree| {
        if let Some(ids) = wanted.get(&(entry.base.clone(), tree)) {
            for id in ids {
                result
                    .entry((*id).to_string())
                    .or_insert_with(|| entry.id.clone());
            }
        }
        result.len() < commit_ids.len() && entry.time >= floor
    };
    walk_chain_while(repo, &ref_name, &mut anchor)?;
    Ok(result)
}
