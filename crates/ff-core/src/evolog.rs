//! The evolution log: one branch's operations, newest first, filtered down to
//! the ones that carry nothing but a working tree.
//!
//! Captures only, and that is the faithful reading of what this view has
//! always shown. `ff evolog` is the timeline of the open change; a verb
//! operation is a thing that *happened to* the change, and `ff op log` is
//! where those live. Showing both here would also break the one property the
//! `@` row rests on: its letters must not move because the user described the
//! change, and a describe operation would become the newest row if the filter
//! were dropped.
//!
//! The walk follows `fufu-prev-branch` — a stated link, never a parent slot.

use gix::prelude::ObjectIdExt;
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::model::{HeadState, OpenChange, SnapEntry};
use crate::ops::message::SegmentLink;
use crate::ops::{BRANCH_PREFIX, OpLog, walk};
use crate::snapshot::chain;

#[derive(Debug, Clone, Default)]
pub struct EvologOptions {
    /// Maximum number of capture rows.
    pub limit: Option<usize>,
    /// Branch to walk (branch name or `@detached`); `None` = HEAD's branch.
    pub chain: Option<String>,
}

/// One branch's captures, newest first. An unreadable link terminates the
/// walk silently — a damaged log shows what is legible.
pub fn evolog(repo: &gix::Repository, opts: &EvologOptions) -> Result<Vec<SnapEntry>> {
    let branch = match &opts.chain {
        Some(name) => name.clone(),
        None => chain::chain_name(&crate::head::head_state(repo)?),
    };
    let mut rows = Vec::new();
    walk_captures(repo, &branch, &mut |decoded| {
        rows.push(decoded.entry);
        !opts.limit.is_some_and(|n| rows.len() >= n)
    })?;
    fill_short_ids(repo, &mut rows);
    Ok(rows)
}

/// Every capture id on one branch, in walk order (newest first), no limit.
/// A resolution domain in its materialized form — ids only, no abbreviation,
/// which is what makes it affordable over a whole branch.
pub fn ref_ids(repo: &gix::Repository, branch: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    walk_captures(repo, branch, &mut |decoded| {
        ids.push(decoded.entry.id);
        true
    })?;
    Ok(ids)
}

/// Abbreviate the ids of rows that are about to be shown or serialized, so
/// the walk itself stays a walk: `short_id` is left empty there and only
/// filled here, for the rows on screen.
///
/// The length comes from the id index rather than from `shorten()`, which is
/// the same answer `ff op log` gives and a cheaper one. `shorten()` asks the
/// object store how short a prefix can be while staying unambiguous among
/// *every object in the repository*, so it grows with the store: twenty-five
/// rows on a thousand-operation log cost four times what they cost on a
/// hundred, for an abbreviation nobody typed differently. The index answers
/// the question actually being asked — unambiguous among the operations —
/// with a binary search over a sorted file.
fn fill_short_ids(repo: &gix::Repository, rows: &mut [SnapEntry]) {
    let hex: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let Ok(lens) = crate::ops::index::prefix_lens(repo, &hex) else {
        // A derived cache that cannot be read is not a reason to fail a read:
        // fall back to the width every other row would have got anyway.
        for row in rows {
            row.short_id = row.id.chars().take(8).collect();
        }
        return;
    };
    for row in rows {
        let len = lens.get(&row.id).copied().unwrap_or(8).max(4);
        row.short_id = row.id.chars().take(len).collect();
    }
}

/// The open change: HEAD's branch summarized as one row.
///
/// Two different questions, two different answers, and conflating them was
/// the bug waiting here. The row's *identity* is the newest capture, because
/// that is the id `ff restore --at` and the `●` anchor column both name, and
/// it must not move when a verb runs. Whether the change is *clean* is the
/// newest operation of any kind, because every operation carries the working
/// tree it leaves behind — asking a capture would call a dirty tree clean in
/// a repository whose only operations so far are verbs.
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

    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let log = OpLog::open(repo)?;
    let has_worktree = repo.workdir().is_some();

    // The newest operation's tree IS the working tree the log last stated.
    let tip_tree: Option<gix::ObjectId> = if has_worktree {
        match log.branch_tip(&branch)? {
            Some(id) => Some(log.get(id)?.tree()),
            None => None,
        }
    } else {
        None
    };
    let clean = tip_tree.is_none_or(|tree| tree == head_tree);

    // The newest capture is the row's identity.
    let mut newest_capture: Option<(String, i64)> = None;
    if has_worktree {
        walk_captures(repo, &branch, &mut |decoded| {
            newest_capture = Some((decoded.entry.id, decoded.entry.time));
            false
        })?;
    }
    let (id, time) = match newest_capture {
        Some((id, time)) => (Some(id), Some(time)),
        None => (None, None),
    };

    // Compute pending hash — any failure → None, open_change must not gain
    // new failure modes.
    let pending = (|| {
        repo.workdir()?;
        if clean && subject.is_none() {
            return None;
        }
        let msg = crate::close::normalize_message(subject.as_deref().unwrap_or(""));
        if !clean {
            // Dirty — an op exists. Tree = what it left behind; timestamp =
            // the newest capture's, or the op's when there is no capture.
            let when = time.or_else(|| {
                log.branch_tip(&branch)
                    .ok()
                    .flatten()
                    .and_then(|id| log.get(id).ok())
                    .map(|op| op.time())
            })?;
            return pending_commit_hash(
                repo,
                tip_tree?,
                base.as_deref()
                    .and_then(|b| gix::ObjectId::from_hex(b.as_bytes()).ok()),
                &msg,
                when,
            );
        }
        // Clean + subject.is_some() — pending empty commit.
        if let Some(tip_time) = time {
            Some(pending_commit_hash(
                repo,
                head_tree,
                base.as_deref()
                    .and_then(|b| gix::ObjectId::from_hex(b.as_bytes()).ok()),
                &msg,
                tip_time,
            )?)
        } else if let Some(base_hex) = &base {
            // No capture yet, HEAD born — use HEAD's own commit time.
            let head_commit_id = gix::ObjectId::from_hex(base_hex.as_bytes()).ok()?;
            let head_commit = repo.find_commit(head_commit_id).ok()?;
            let head_time = head_commit.time().ok()?.seconds;
            Some(pending_commit_hash(
                repo,
                head_tree,
                Some(head_commit_id),
                &msg,
                head_time,
            )?)
        } else {
            // No capture, unborn — no timestamp source.
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

/// One decoded operation in display shape, plus the two walk edges that don't
/// belong in a public, serialized row: `next`, the previous operation on this
/// branch, and `segment_prev`, the segment skip-link (see [`segment_anchors`]).
pub(crate) struct SnapDecode {
    pub entry: SnapEntry,
    pub next: Option<gix::ObjectId>,
    pub tree: gix::ObjectId,
    pub is_capture: bool,
    pub segment_prev: Option<SegmentLink>,
}

/// Decode one operation into its walk shape. `None` when the id is not an
/// operation at all — a hand-pointed ref terminates a walk rather than
/// failing it.
pub(crate) fn snap_entry(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<SnapDecode>> {
    let Ok(op) = walk::decode(repo, id) else {
        return Ok(None);
    };
    let next = op.prev_on_branch().map(|p| p.object_id());
    // The row's `prev` is the previous *capture*, so the edges chain the rows
    // this view actually shows. Usually one step; a run of verb operations
    // between two captures is short by construction.
    let prev = prev_capture(repo, next)?;
    let entry = SnapEntry {
        id: id.to_string(),
        // Filled by `fill_short_ids` on the display paths only.
        short_id: String::new(),
        subject: op.summary().to_string(),
        time: op.time(),
        base: op.base().map(|b| b.object_id().to_string()),
        prev: prev.map(|p| p.to_string()),
    };
    Ok(Some(SnapDecode {
        entry,
        next,
        tree: op.tree(),
        is_capture: op.is_capture(),
        segment_prev: op.prev_segment(),
    }))
}

/// The first capture at or below `from`, following the branch link.
fn prev_capture(
    repo: &gix::Repository,
    from: Option<gix::ObjectId>,
) -> Result<Option<gix::ObjectId>> {
    let mut cur = from;
    while let Some(id) = cur {
        let Ok(op) = walk::decode(repo, id) else {
            return Ok(None);
        };
        if op.is_capture() {
            return Ok(Some(id));
        }
        cur = op.prev_on_branch().map(|p| p.object_id());
    }
    Ok(None)
}

/// Walk one branch newest-first, handing every *capture* to `visit`. The walk
/// ends when `visit` returns false, when the branch runs out, or when a link
/// leaves the log. Every capture walk goes through here, so "how far do we
/// walk" is one decision per caller rather than a habit.
fn walk_captures(
    repo: &gix::Repository,
    branch: &str,
    visit: &mut dyn FnMut(SnapDecode) -> bool,
) -> Result<()> {
    let Some(tip) = crate::refs::ref_target(repo, &format!("{BRANCH_PREFIX}{branch}"))? else {
        return Ok(());
    };
    let mut cur = Some(tip);
    while let Some(id) = cur {
        let Some(decoded) = snap_entry(repo, id)? else {
            break;
        };
        cur = decoded.next;
        if decoded.is_capture && !visit(decoded) {
            break;
        }
    }
    Ok(())
}

/// How many operations the anchor walk will decode one at a time — scanning
/// inside a segment whose base is wanted, or stepping the plain branch link
/// when a segment pointer is missing or untrustworthy — before it gives up on
/// further linear stepping and only continues by hopping validated pointers.
/// Exceeding it costs a row its drill-in letters, nothing more.
const SEGMENT_SCAN_CAP: usize = 512;

/// For each displayed commit id (full hex), the newest capture on the branch
/// whose base is the commit's first parent and whose tree equals the commit's
/// tree — the evolog drill-in anchor. Root commits match base-less captures.
/// Commits with no match are absent from the map.
///
/// **Only captures answer.** A verb operation records a planned end state, and
/// a close's plan is exactly "base = the old HEAD, tree = the tree I am about
/// to commit" — which matches the commit it creates on both axes and would
/// shadow, forever, the capture that actually recorded the user's work. The
/// anchor is meant to be the moment the content existed in the working tree,
/// not the moment fufu wrote it down.
///
/// Operations form contiguous segments: everything recorded while HEAD sat at
/// commit B has `base = B`, and a segment ends the moment HEAD moves. An
/// anchor can only be found in a segment whose base some displayed commit is
/// asking for, so a segment whose base nobody wants is skipped outright by
/// hopping its `fufu-prev-segment` pointer straight to the previous segment's
/// newest operation — O(1), regardless of how many operations the skipped
/// segment holds. A wanted segment is scanned inside, newest-first, capped at
/// `SEGMENT_SCAN_CAP` so one enormous segment (the open change, which grows
/// with every capture between commits) can't reintroduce the O(log depth)
/// cost this walk exists to avoid.
///
/// The pointer is a hint, never authority (state is a rebuildable cache over
/// git; the repository wins on disagreement) — see `resolve_hop`.
pub fn segment_anchors(
    repo: &gix::Repository,
    commit_ids: &[String],
) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();
    if commit_ids.is_empty() {
        return Ok(result);
    }
    let head = crate::head::head_state(repo)?;
    let branch = chain::chain_name(&head);

    // What the displayed commits are asking, keyed the way an operation
    // answers: (base, tree). Two commits can share a key — an empty commit
    // beside the one whose tree it repeats — so a key answers a list.
    let mut wanted: HashMap<(Option<String>, gix::ObjectId), Vec<&str>> = HashMap::new();
    // An operation cannot be based on a commit that did not exist when it ran,
    // so nothing below the oldest base's time can anchor anything here and the
    // walk stops there. A base we cannot read (a root commit, a shallow
    // boundary) drops the floor away rather than risk a lost anchor.
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
    // Just the bases, for the O(1) "is this segment worth scanning at all"
    // check — a segment is skipped outright the moment its base isn't in here,
    // without looking at any of its trees.
    let wanted_bases: std::collections::HashSet<Option<String>> =
        wanted.keys().map(|(base, _)| base.clone()).collect();

    let Some(tip) = crate::refs::ref_target(repo, &format!("{BRANCH_PREFIX}{branch}"))? else {
        return Ok(result);
    };
    let Some(mut cur) = snap_entry(repo, tip)? else {
        return Ok(result);
    };

    // Spent scanning operation-by-operation: inside a wanted segment looking
    // for a tree match, or as the fallback when a pointer is absent or fails
    // validation. A validated hop never spends it — that's the O(1) skip that
    // makes this walk O(segments), not O(operations).
    let mut linear_budget = SEGMENT_SCAN_CAP;

    loop {
        // Record this operation's match (if any) before checking either stop
        // condition below — matches the full sweep's order exactly.
        if cur.is_capture
            && let Some(ids) = wanted.get(&(cur.entry.base.clone(), cur.tree))
        {
            for id in ids {
                result
                    .entry((*id).to_string())
                    .or_insert_with(|| cur.entry.id.clone());
            }
        }
        if result.len() >= commit_ids.len() || cur.entry.time < floor {
            break;
        }

        // Prefer a validated hop once this segment stops being worth scanning
        // operation-by-operation: either its base was never wanted (skip it
        // outright), or the budget for stepping through it ran out.
        let scan_further = wanted_bases.contains(&cur.entry.base) && linear_budget > 0;
        if !scan_further {
            match cur.segment_prev {
                Some(SegmentLink::At(ptr))
                    if let Some(hopped) = resolve_hop(repo, ptr, cur.entry.time) =>
                {
                    cur = hopped;
                    continue;
                }
                Some(SegmentLink::ChainStart) => {
                    // Everything below is in this same first segment — same
                    // base, not one any displayed commit wants.
                    break;
                }
                _ => {}
            }
        }

        // No hop taken — step the plain branch link, spending budget.
        let Some(next_id) = cur.next else { break };
        if linear_budget == 0 {
            break;
        }
        linear_budget -= 1;
        let Some(next) = snap_entry(repo, next_id)? else {
            break;
        };
        cur = next;
    }
    Ok(result)
}

/// Validate a segment pointer before trusting it, and decode its target in the
/// same step so a successful hop costs exactly one object read. The pointer
/// must resolve to an actual operation no newer than the one it was read from;
/// any failure is treated as no pointer at all, never as an error and never as
/// a reason to end the walk.
///
/// A pointer that checks out but is merely *stale* is still safe to hop to. An
/// anchor is only ever accepted on an exact `(base, tree)` match, so landing in
/// stale or abandoned history can only cost precision: the tree the returned id
/// names is still exactly the displayed commit's tree.
fn resolve_hop(
    repo: &gix::Repository,
    ptr: gix::ObjectId,
    not_newer_than: i64,
) -> Option<SnapDecode> {
    match snap_entry(repo, ptr) {
        Ok(Some(decoded)) if decoded.entry.time <= not_newer_than => Some(decoded),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
