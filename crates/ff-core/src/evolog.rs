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
//! The `@` row's sha is the open commit's — the commit under
//! `refs/fufu/open/<branch>` that the close moves the branch to, see
//! [`crate::open`] — read from the ref, never predicted.
//!
//! The walk follows `fufu-prev-branch` — a stated link, never a parent slot.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::model::{ChangeHistory, ChangeOp, HeadState, OpenChange, SnapEntry};
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
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => (
            Some(commit.clone()),
            Some(crate::sha::short(commit).to_string()),
        ),
    };
    // The pending description is advisory in a read: a lookup that cannot
    // run is a missing line, never a failed one — the verbs that consume the
    // description read it strictly, themselves.
    let meta = crate::branchmeta::read(repo, &branch).ok();
    let subject = meta
        .as_ref()
        .and_then(|meta| meta.pending_description.clone());
    // The identity: what was minted for the open change, or, inside an
    // editing session, the commit being amended, whose id the landing keeps.
    let change_id = match meta.as_ref().and_then(|meta| meta.session.as_ref()) {
        Some(session) => gix::ObjectId::from_hex(session.at.as_bytes())
            .ok()
            .and_then(|at| repo.find_commit(at).ok())
            .map(|commit| crate::changeid::of_commit(&commit.data, &commit.id).letters()),
        None => meta.as_ref().and_then(|meta| meta.change_id.clone()),
    };

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

    // The open commit's sha, when the ref still describes the branch. Under
    // signing the column stays blank: the object and the ref exist, but the
    // close signs, and the sha it lands is not this one. Advisory, like the
    // description: a read that cannot answer is a blank, never a failure.
    let pending = if crate::sign::enabled(repo) {
        None
    } else {
        tip_tree
            .and_then(|tree| {
                let head_commit = base
                    .as_deref()
                    .and_then(|b| gix::ObjectId::from_hex(b.as_bytes()).ok());
                crate::open::current(repo, &branch, tree, head_commit).ok()
            })
            .flatten()
            .map(|id| id.to_string())
    };

    Ok(OpenChange {
        branch,
        id,
        change_id,
        base,
        base_short,
        subject,
        time,
        clean,
        pending,
    })
}

/// A commit's history as a change: `ff evolog <rev>`.
///
/// A commit with a `change-id` header was made by fufu, and the operation
/// log knows what happened to it: every chain — each worktree's, including
/// chains whose worktree is gone — is read for the verb operations whose
/// rewrites or ref moves produced a commit carrying the same id. The close
/// (`verb == "commit"`) also contributes its segment's captures on its own
/// chain: the captures taken while HEAD sat on the close's base, which is
/// the work the commit closed.
///
/// A commit without the header has no operations to find — the id is derived
/// from the sha, and nothing wrote it — so the fallback is the anchor walk
/// the column used to draw: the current chain's capture matching the commit,
/// and that segment's captures back to its boundary.
pub fn evolog_of(
    repo: &gix::Repository,
    sha: gix::ObjectId,
    limit: Option<usize>,
) -> Result<ChangeHistory> {
    let commit = repo.find_commit(sha).map_err(Error::repo)?;
    let header = crate::changeid::header_of(&commit.data);
    let id = header.unwrap_or_else(|| crate::changeid::ChangeId::derive(&sha));
    let mut history = ChangeHistory {
        change_id: id.letters(),
        commit: sha.to_string(),
        operations: Vec::new(),
        snapshots: Vec::new(),
    };

    let Some(id) = header else {
        history.snapshots = segment_captures(repo, sha)?;
        return Ok(history);
    };

    // Every chain, this worktree's included even when it has no ops yet.
    let mut chains = crate::ops::chain_ids(repo)?;
    let me = crate::ops::chain_id(repo);
    if !chains.contains(&me) {
        chains.push(me);
    }
    // A sha is asked about once, however many operations name it: the
    // answer is a property of the commit, not of the operation.
    let mut carries: HashMap<String, bool> = HashMap::new();
    let mut carries_id = |hex: &str| -> bool {
        if let Some(&known) = carries.get(hex) {
            return known;
        }
        let known = gix::ObjectId::from_hex(hex.as_bytes())
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
            .is_some_and(|c| crate::changeid::of_commit(&c.data, &c.id) == id);
        carries.insert(hex.to_string(), known);
        known
    };
    let mut close: Option<(String, walk::Operation<'_>)> = None;
    // Each operation with the commits it rewrote, for the ordering below.
    let mut found: Vec<(ChangeOp, Vec<String>)> = Vec::new();
    for chain in &chains {
        let log = OpLog::open_chain(repo, chain.clone())?;
        for op in log.iter_verbs() {
            // A damaged chain shows what is legible, like every other walk.
            let Ok(op) = op else { break };
            if op.is_capture() {
                continue;
            }
            let Some(record) = op.record()? else {
                continue;
            };
            let produced = record
                .rewrites
                .iter()
                .map(|r| r.new.as_str())
                .chain(record.refs.iter().filter_map(|t| t.new.as_deref()))
                .find(|hex| carries_id(hex))
                .map(str::to_string);
            let Some(produced) = produced else {
                continue;
            };
            let rewrote: Vec<String> = record.rewrites.iter().map(|r| r.old.clone()).collect();
            found.push((
                ChangeOp {
                    id: op.id().to_string(),
                    short_id: String::new(),
                    chain: chain.clone(),
                    verb: record.verb.clone(),
                    summary: op.summary().to_string(),
                    time: op.time(),
                    commit: produced,
                    session: op.session().map(str::to_string),
                },
                rewrote,
            ));
            if record.verb == "commit" && close.is_none() {
                close = Some((chain.clone(), op));
            }
        }
    }
    // Newest first. Clocks have one-second grain and chains have no shared
    // order, so two operations can tie; the rewrite edge settles a tie — an
    // operation that rewrote what another produced came after it.
    found.sort_by(|a, b| b.0.time.cmp(&a.0.time).then_with(|| a.0.id.cmp(&b.0.id)));
    let mut settled = true;
    while settled {
        settled = false;
        for i in 1..found.len() {
            let ahead = &found[i - 1];
            let behind = &found[i];
            if ahead.0.time == behind.0.time && behind.1.contains(&ahead.0.commit) {
                found.swap(i - 1, i);
                settled = true;
            }
        }
    }
    history.operations = found.into_iter().map(|(op, _)| op).collect();
    if let Some(n) = limit {
        history.operations.truncate(n);
    }
    fill_op_short_ids(repo, &mut history.operations);

    if let Some((_, close)) = close {
        // The close's segment: the captures behind it on its own chain,
        // walked from the operation before the close while the base holds.
        let base = close.base().map(|b| b.object_id().to_string());
        let mut cur = close.prev_on_branch().map(|p| p.object_id());
        while let Some(op_id) = cur {
            let Some(decoded) = snap_entry(repo, op_id)? else {
                break;
            };
            if decoded.entry.base != base {
                break;
            }
            cur = decoded.next;
            if decoded.is_capture {
                history.snapshots.push(decoded.entry);
            }
        }
        fill_short_ids(repo, &mut history.snapshots);
    }
    Ok(history)
}

/// The captures of the current chain's segment matching a commit fufu did
/// not close: the anchor the column used to draw, and every capture below
/// it on the same base.
fn segment_captures(repo: &gix::Repository, sha: gix::ObjectId) -> Result<Vec<SnapEntry>> {
    let hex = sha.to_string();
    let mut rows = Vec::new();
    let Some(anchor) = segment_anchors(repo, std::slice::from_ref(&hex))?.remove(&hex) else {
        return Ok(rows);
    };
    let anchor = gix::ObjectId::from_hex(anchor.as_bytes()).map_err(Error::repo)?;
    let Some(first) = snap_entry(repo, anchor)? else {
        return Ok(rows);
    };
    let base = first.entry.base.clone();
    let mut cur = Some(first);
    while let Some(decoded) = cur.take() {
        if decoded.entry.base != base {
            break;
        }
        let next = decoded.next;
        if decoded.is_capture {
            rows.push(decoded.entry);
        }
        cur = match next {
            Some(id) => snap_entry(repo, id)?,
            None => None,
        };
    }
    fill_short_ids(repo, &mut rows);
    Ok(rows)
}

/// `fill_short_ids` for the operation rows: the same index, the same
/// fallback, over the same hex.
fn fill_op_short_ids(repo: &gix::Repository, rows: &mut [ChangeOp]) {
    let hex: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let lens = crate::ops::index::prefix_lens(repo, &hex).ok();
    for row in rows {
        let len = lens
            .as_ref()
            .and_then(|lens| lens.get(&row.id).copied())
            .unwrap_or(8)
            .max(4);
        row.short_id = row.id.chars().take(len).collect();
    }
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
/// Exceeding it costs a row its drill-in id, nothing more.
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
