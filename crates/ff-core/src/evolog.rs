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

/// One decoded snapshot commit: the display-shaped [`SnapEntry`] plus the two
/// walk edges that don't belong in a public, serialized row — `next`, the
/// linear predecessor to continue a plain walk with, and `segment_prev`, the
/// segment skip-link (see `segment_anchors`). Capture reads `segment_prev`
/// and `entry.base` through this same decode to mint the next snapshot's own
/// pointer, so the parsing of "what does this commit's parents and message
/// mean" lives in exactly one place.
pub(crate) struct SnapDecode {
    pub entry: SnapEntry,
    pub next: Option<gix::ObjectId>,
    pub tree: gix::ObjectId,
    /// What the snapshot knows about the segment before its own. `None` on a
    /// chain written before the trailer existed (unknown); `ChainStart` means
    /// this segment is the first in the chain; `At(oid)` names the newest
    /// snapshot of the previous segment.
    pub segment_prev: Option<crate::snapshot::message::SegmentPrev>,
}

/// Decode a snapshot commit into its walk shape.
pub(crate) fn snap_entry(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<SnapDecode>> {
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
    let segment_prev = commit.message().body.and_then(|body| {
        crate::snapshot::message::parse_segment_prev(&String::from_utf8_lossy(body.as_ref()))
    });
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
    Ok(Some(SnapDecode {
        entry,
        next: prev,
        tree,
        segment_prev,
    }))
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
        let Some(decoded) = snap_entry(repo, id)? else {
            break;
        };
        if !visit(decoded.entry, decoded.tree) {
            break;
        }
        cur = decoded.next;
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

/// How many snapshots the anchor walk will decode one at a time — scanning
/// inside a segment whose base is wanted, or stepping the plain chain link
/// when a segment pointer is missing or untrustworthy — before it gives up
/// on further linear stepping and only continues by hopping validated
/// pointers. Exceeding it costs a row its drill-in letters, nothing more: the
/// commit just prints without one, the same degradation the floor already
/// causes for a faked-future committer date. 512 is generous for the
/// dominant case (the newest snapshot in a wanted segment is the match) while
/// still bounding the pathological one (thousands of same-base, all-different
/// captures piled up in one open-change segment).
const SEGMENT_SCAN_CAP: usize = 512;

/// For each displayed commit id (full hex), the newest live-chain snapshot
/// whose base is the commit's first parent and whose tree equals the commit's
/// tree — the evolog drill-in anchor. Root commits match base-less snapshots.
/// Commits with no match are absent from the map. Trash chains never contribute.
///
/// Snapshots form contiguous segments: every snapshot taken while HEAD sat at
/// commit B has `base = B`, and a segment ends the moment HEAD moves. An
/// anchor can only be found in a segment whose base some displayed commit is
/// asking for, so a segment whose base nobody wants is skipped outright by
/// hopping its `segment_prev` pointer straight to the previous segment's
/// newest snapshot — O(1), regardless of how many snapshots the skipped
/// segment holds. A wanted segment is scanned inside, newest-first (tree
/// varies within a segment even though base doesn't), capped at
/// `SEGMENT_SCAN_CAP` so one enormous segment (the open change, which grows
/// with every capture between commits) can't reintroduce the O(chain depth)
/// cost this walk exists to avoid.
///
/// A chain written before this pointer existed carries none: every snapshot
/// on it decodes with `segment_prev = None`, so the walk falls back to the
/// plain chain link one snapshot at a time from there — capped by the same
/// budget, so an old, unhealed chain degrades to "this row loses its
/// letters" rather than to the full linear sweep this walk replaces. New
/// captures always carry the pointer, so a chain heals from the tip down as
/// they accumulate: derived and disposable, the same stance the id index
/// takes toward its own file.
///
/// The pointer is a hint, never authority (state is a rebuildable cache over
/// git; the repository wins on disagreement) — see `resolve_hop` for the
/// validation that keeps a stale or wrong trailer from ever corrupting the
/// walk, and the comment there for why a merely *stale* one is still safe to
/// use.
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
    // Just the bases, for the O(1) "is this segment worth scanning at all"
    // check — a segment is skipped outright the moment its base isn't in
    // here, without looking at any of its trees.
    let wanted_bases: std::collections::HashSet<Option<String>> =
        wanted.keys().map(|(base, _)| base.clone()).collect();

    let Some(tip) = chain::tip(repo, &ref_name)? else {
        return Ok(result);
    };
    let Some(mut cur) = snap_entry(repo, tip)? else {
        return Ok(result);
    };

    // Spent scanning snapshot-by-snapshot: inside a wanted segment looking
    // for a tree match, or as the fallback when a pointer is absent or fails
    // validation. A validated hop never spends it — that's the O(1) skip
    // that makes this walk O(segments), not O(snapshots), on a healed chain.
    let mut linear_budget = SEGMENT_SCAN_CAP;

    loop {
        // Record this snapshot's match (if any) before checking either stop
        // condition below — matches the full sweep's order exactly: a
        // snapshot that happens to sit right at the floor still gets to
        // answer before the walk ends on it.
        if let Some(ids) = wanted.get(&(cur.entry.base.clone(), cur.tree)) {
            for id in ids {
                result
                    .entry((*id).to_string())
                    .or_insert_with(|| cur.entry.id.clone());
            }
        }
        if result.len() >= commit_ids.len() || cur.entry.time < floor {
            break;
        }

        // Prefer a validated hop once this segment stops being worth
        // scanning snapshot-by-snapshot: either its base was never wanted
        // (skip it outright), or it was wanted but the budget for stepping
        // through it ran out (give up on the rest of it, same as any other
        // budget exhaustion).
        let scan_further = wanted_bases.contains(&cur.entry.base) && linear_budget > 0;
        if !scan_further {
            match cur.segment_prev {
                Some(crate::snapshot::message::SegmentPrev::At(ptr))
                    if let Some(hopped) = resolve_hop(repo, ptr, cur.entry.time) =>
                {
                    cur = hopped;
                    continue;
                }
                Some(crate::snapshot::message::SegmentPrev::ChainStart) => {
                    // Every snapshot below is in this same first segment —
                    // same base, not one any displayed commit wants.
                    break;
                }
                _ => {}
            }
        }

        // No hop taken — step the plain chain link, spending budget. This is
        // the within-segment scan (segment wanted) and the pointerless
        // fallback (old chain, or a hop that failed validation) alike.
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

/// Validate a segment pointer before trusting it, and decode its target in
/// the same step so a successful hop costs exactly one object read. The
/// pointer must resolve to an actual fufu snapshot commit no newer than the
/// snapshot it was read from; any failure — missing object, wrong kind, not
/// a snapshot, or (should a trailer ever survive corrupted) a target that is
/// somehow younger — is treated as no pointer at all, never as an error and
/// never as a reason to end the walk. State here is a cache over git, not
/// authority over it, so a hint that doesn't check out just means falling
/// back to the plain chain link, exactly as if the trailer had never been
/// written.
///
/// A pointer that checks out but is merely *stale* — it names a snapshot
/// that isn't actually the newest match anymore, e.g. after `ff trim`
/// dropped what used to sit between it and its target — is still safe to
/// hop to. An anchor is only ever accepted on an exact `(base, tree)` match,
/// so landing in stale or even abandoned history can only ever cost
/// precision (an older match than the full sweep would have found, or none):
/// the tree the returned id names is still exactly the displayed commit's
/// tree, so `ff restore --at` on it produces identical content either way.
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
mod tests {
    use super::*;
    use crate::snapshot::message::SegmentPrev;
    use crate::snapshot::{self, Provenance, TakeOptions};
    use crate::trim::{self, TrimOptions};
    use ff_testsupport::Fixture;

    fn take_created(fx: &Fixture) -> String {
        let repo = fx.repo();
        match snapshot::take(&repo, &Provenance::new("manual", None)).expect("take") {
            crate::model::SnapOutcome::Created { id, .. } => id,
            other => panic!("expected Created, got {other:?}"),
        }
    }

    /// A snapshot backdated `days_ago` from `now`, for trim fixtures.
    fn snap_at(fx: &Fixture, now: i64, days_ago: i64) -> String {
        let repo = fx.repo();
        match snapshot::take_with(
            &repo,
            &Provenance::new("manual", None),
            &TakeOptions {
                now: Some(now - days_ago * 86_400),
                max_file_size: None,
            },
        )
        .expect("take")
        {
            crate::model::SnapOutcome::Created { id, .. } => id,
            other => panic!("expected Created, got {other:?}"),
        }
    }

    fn ref_name(repo: &gix::Repository) -> String {
        let head = crate::head::head_state(repo).unwrap();
        format!("{}{}", chain::SNAP_PREFIX, chain::chain_name(&head))
    }

    fn decode(repo: &gix::Repository, id: &str) -> SnapDecode {
        snap_entry(repo, gix::ObjectId::from_hex(id.as_bytes()).unwrap())
            .unwrap()
            .expect("id decodes as a snapshot")
    }

    /// The pre-skip-link algorithm, kept here only as a correctness oracle:
    /// a full linear walk with no hopping and no cap, exactly what
    /// `segment_anchors` did before the segment pointer existed. The new
    /// walk must agree with this on every id it manages to reach within its
    /// scan budget — this is the property under test throughout this module.
    fn full_sweep(repo: &gix::Repository, commit_ids: &[String]) -> HashMap<String, String> {
        let mut result = HashMap::new();
        if commit_ids.is_empty() {
            return result;
        }
        let mut wanted: HashMap<(Option<String>, gix::ObjectId), Vec<&str>> = HashMap::new();
        let mut floor: Option<i64> = Some(i64::MAX);
        for id in commit_ids {
            let oid = gix::ObjectId::from_hex(id.as_bytes()).unwrap();
            let commit = repo.find_commit(oid).unwrap();
            let parent = commit.parent_ids().next().map(|p| p.detach());
            let tree = commit.tree_id().unwrap().detach();
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
        walk_chain_while(repo, &ref_name(repo), &mut anchor).unwrap();
        result
    }

    #[test]
    fn skip_link_matches_full_sweep_across_segments_and_a_root() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        let c0 = fx.commit("init"); // root: no base at all
        fx.write("a.txt", "one\n");
        take_created(&fx);
        fx.write("a.txt", "two\n");
        let s2 = take_created(&fx); // segment 1 (base c0): s1, s2
        let c1 = fx.commit("second"); // tree == s2's tree, parent == c0
        fx.write("a.txt", "three\n");
        take_created(&fx);
        fx.write("a.txt", "four\n");
        let s4 = take_created(&fx); // segment 2 (base c1): s3, s4
        let c2 = fx.commit("third"); // tree == s4's tree, parent == c1
        fx.write("a.txt", "five\n");
        let s5 = take_created(&fx); // segment 3 (base c2): s5 alone
        let c3 = fx.commit("fourth"); // tree == s5's tree, parent == c2

        let repo = fx.repo();
        let ids = vec![c0.clone(), c1.clone(), c2.clone(), c3.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(fast, slow, "skip-link walk must agree with the full sweep");

        assert_eq!(fast.get(&c1), Some(&s2));
        assert_eq!(fast.get(&c2), Some(&s4));
        assert_eq!(fast.get(&c3), Some(&s5));
        assert!(
            !fast.contains_key(&c0),
            "a root commit has no base to match against"
        );
    }

    #[test]
    fn segment_pointer_assignment_follows_the_rule() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "one\n");
        let s1 = take_created(&fx);
        fx.write("a.txt", "two\n");
        let s2 = take_created(&fx);
        fx.commit("second"); // base moves: next capture opens a new segment
        fx.write("a.txt", "three\n");
        let s3 = take_created(&fx);
        fx.write("a.txt", "four\n");
        let s4 = take_created(&fx);

        let repo = fx.repo();
        assert_eq!(
            decode(&repo, &s1).segment_prev,
            Some(SegmentPrev::ChainStart),
            "the first snapshot of a chain declares itself chain start"
        );
        assert_eq!(
            decode(&repo, &s2).segment_prev,
            Some(SegmentPrev::ChainStart),
            "same segment as s1: copies its ChainStart verbatim"
        );
        let s3_decoded = decode(&repo, &s3);
        assert_eq!(
            s3_decoded.segment_prev,
            Some(SegmentPrev::At(
                gix::ObjectId::from_hex(s2.as_bytes()).unwrap()
            )),
            "a fresh segment points at prev itself"
        );
        let s4_decoded = decode(&repo, &s4);
        assert_eq!(
            s4_decoded.segment_prev, s3_decoded.segment_prev,
            "same segment as s3: copies its pointer verbatim"
        );
    }

    #[test]
    fn raw_git_commit_without_a_snapshot_has_no_anchor() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "one\n");
        let s1 = take_created(&fx);
        let c1 = fx.commit("second"); // tree == s1's tree, parent == c0: has an anchor
        fx.write("a.txt", "two\n");
        let c2 = fx.commit("third"); // committed straight through git: no snapshot ever matches it

        let repo = fx.repo();
        let ids = vec![c1.clone(), c2.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(fast, slow);
        assert_eq!(fast.get(&c1), Some(&s1));
        assert!(
            !fast.contains_key(&c2),
            "no capture ever recorded c2's content"
        );
    }

    /// Rewrite the oldest `n` snapshots on a chain to drop their segment
    /// pointer trailer — simulating a chain whose earliest history predates
    /// this feature — relinking parent 1 through every snapshot above them
    /// so the chain stays one valid first-parent history (the same
    /// relinking `trim` does, minus the age cutoff). Returns the original id
    /// -> rewritten id map for every snapshot on the chain, since content
    /// addressing means a relinked parent changes a commit's sha even where
    /// its own message does not.
    fn strip_pointers_from_oldest(
        repo: &gix::Repository,
        ref_name: &str,
        n: usize,
    ) -> HashMap<String, gix::ObjectId> {
        let newest_first = ref_ids(repo, ref_name).unwrap();
        let mut mapping = HashMap::new();
        let mut prev_new: Option<gix::ObjectId> = None;
        for (i, old_hex) in newest_first.iter().rev().enumerate() {
            let old_id = gix::ObjectId::from_hex(old_hex.as_bytes()).unwrap();
            let obj = repo.find_object(old_id).unwrap();
            let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).unwrap();
            let mut commit: gix::objs::Commit = commit_ref.into();
            drop(obj);
            if let Some(p) = prev_new
                && !commit.parents.is_empty()
            {
                commit.parents[0] = p;
            }
            if i < n {
                let text = String::from_utf8_lossy(commit.message.as_ref()).into_owned();
                commit.message = crate::snapshot::message::rewrite_segment_prev(&text, None).into();
            }
            let new_id = repo.write_object(&commit).unwrap().detach();
            mapping.insert(old_hex.clone(), new_id);
            prev_new = Some(new_id);
        }
        let tip = prev_new.expect("chain is non-empty");
        crate::refs::write_ref(
            repo,
            ref_name,
            tip,
            gix::refs::transaction::PreviousValue::Any,
            0,
            "test: simulate a pre-pointer chain",
        )
        .unwrap();
        mapping
    }

    #[test]
    fn mixed_chain_pointerless_prefix_falls_back_and_still_finds_anchors() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "one\n");
        take_created(&fx);
        fx.write("a.txt", "two\n");
        let s2 = take_created(&fx); // segment 1 (base c0)
        let c1 = fx.commit("second"); // tree == s2's tree
        fx.write("a.txt", "three\n");
        take_created(&fx);
        fx.write("a.txt", "four\n");
        let s4 = take_created(&fx); // segment 2 (base c1)
        let c2 = fx.commit("third"); // tree == s4's tree

        let repo = fx.repo();
        let ref_name = ref_name(&repo);

        // Strip every snapshot captured so far — the whole of segments 1
        // and 2 now looks like it was written before this feature existed.
        let mapping = strip_pointers_from_oldest(&repo, &ref_name, 4);
        let new_s2 = mapping[&s2];
        let new_s4 = mapping[&s4];

        // Capture more, normally, on top of the rewritten prefix: this half
        // of the chain gets real pointers.
        fx.write("a.txt", "five\n");
        take_created(&fx);
        fx.write("a.txt", "six\n");
        let s6 = take_created(&fx); // segment 3 (base c2), pointer -> new_s4
        let c3 = fx.commit("fourth"); // tree == s6's tree

        let repo = fx.repo();
        let ids = vec![c1.clone(), c2.clone(), c3.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(
            fast, slow,
            "must agree even where the walk has to fall back"
        );
        assert_eq!(fast.get(&c1), Some(&new_s2.to_string()));
        assert_eq!(fast.get(&c2), Some(&new_s4.to_string()));
        assert_eq!(fast.get(&c3), Some(&s6));

        // The newer half still carries a pointer; the rewritten half does
        // not — a genuine mixed chain, healing from the tip down.
        assert!(decode(&repo, &s6).segment_prev.is_some());
        assert_eq!(decode(&repo, &new_s4.to_string()).segment_prev, None);
        assert_eq!(decode(&repo, &new_s2.to_string()).segment_prev, None);
    }

    #[test]
    fn anchors_after_trim_relink_or_drop_the_pointer() {
        const NOW: i64 = 1_700_000_000;
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "one\n");
        snap_at(&fx, NOW, 100); // base c0 — will be trimmed away (> 90d)
        let c1 = fx.commit("second"); // tree == "one"
        fx.write("a.txt", "two\n");
        snap_at(&fx, NOW, 5); // base c1 — kept; boundary trailer -> the 100d snapshot
        let c2 = fx.commit("third"); // tree == "two"
        fx.write("a.txt", "three\n");
        snap_at(&fx, NOW, 4); // base c2 — kept; boundary trailer -> the 5d snapshot
        let c3 = fx.commit("fourth"); // tree == "three"

        let repo = fx.repo();
        trim::trim(
            &repo,
            &TrimOptions {
                now: Some(NOW),
                dry_run: false,
                gone: false,
                keep_secs: None, // fufu.keep default: 90 days
            },
        )
        .expect("trim");

        let ref_name = ref_name(&repo);
        let survivors = ref_ids(&repo, &ref_name).unwrap(); // newest first
        assert_eq!(survivors.len(), 2, "the 100d snapshot alone was dropped");
        let new_top = survivors[0].clone(); // was the 4d snapshot
        let new_mid = survivors[1].clone(); // was the 5d snapshot

        let ids = vec![c1.clone(), c2.clone(), c3.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(fast, slow);

        assert!(
            !fast.contains_key(&c1),
            "its only anchor was trimmed off the live chain"
        );
        assert_eq!(fast.get(&c2), Some(&new_mid));
        assert_eq!(fast.get(&c3), Some(&new_top));

        // The surviving mid snapshot's trailer named the now-gone 100d
        // snapshot: dropped, not left dangling. The surviving top
        // snapshot's trailer named the mid snapshot, which did survive:
        // relinked to its rewritten id.
        assert_eq!(decode(&repo, &new_mid).segment_prev, None);
        assert_eq!(
            decode(&repo, &new_top).segment_prev,
            Some(SegmentPrev::At(
                gix::ObjectId::from_hex(new_mid.as_bytes()).unwrap()
            ))
        );
    }

    #[test]
    fn single_segment_chain_stops_at_chain_start_sentinel() {
        // A long single-segment chain whose base no displayed commit wants:
        // the walk should stop at the ChainStart sentinel without burning
        // the linear budget on pointless decodes.
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");

        // Create many snapshots on one open change. More than
        // SEGMENT_SCAN_CAP so the old behavior would hit the cap.
        for i in 0..600 {
            fx.write("a.txt", format!("v{}\n", i).as_str());
            take_created(&fx);
        }

        // Commit — this moves the base. The 600-snapshot segment (base =
        // init) is now behind this commit and no displayed commit wants
        // that base.
        fx.write("a.txt", "final\n");
        let c_final = fx.commit("final");
        // One more snapshot in the new segment (base = c_final).
        fx.write("a.txt", "after\n");
        let _s_after = take_created(&fx);

        // Ask for c_final: its first parent is the "init" commit, and
        // s_after has base = c_final, so s_after is NOT the anchor.
        // The 600-snapshot segment has base = init, which IS c_final's
        // parent, so one of those 600 snapshots matches. But the fast
        // walk must reach it by hopping through the new segment, then
        // scanning the old one.
        let repo = fx.repo();
        let ids = vec![c_final.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(fast, slow, "anchors must match the oracle");
    }

    #[test]
    fn mixed_chain_pointerless_prefix_with_chain_start_still_degrades_safely() {
        // Pointerless old snapshots (ChainStart-bearing) below new ones
        // carrying `At` pointers: the walk falls back through the pointerless
        // region and still finds every anchor the oracle finds.
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "one\n");
        take_created(&fx);
        fx.write("a.txt", "two\n");
        let s2 = take_created(&fx); // segment 1 (base c0), ChainStart
        let c1 = fx.commit("second"); // tree == s2's tree
        fx.write("a.txt", "three\n");
        take_created(&fx);
        fx.write("a.txt", "four\n");
        let s4 = take_created(&fx); // segment 2 (base c1), At(s2)
        let c2 = fx.commit("third"); // tree == s4's tree

        let repo = fx.repo();
        let ref_name = ref_name(&repo);

        // Strip the oldest snapshots to simulate a pre-pointer prefix —
        // the first segment loses its trailer. strip_pointers_from_oldest
        // rewrites ALL snapshots to maintain parent chain integrity, so
        // we must use the mapped ids.
        let mapping = strip_pointers_from_oldest(&repo, &ref_name, 2);
        let new_s2 = mapping[&s2];
        let new_s4 = mapping[&s4];

        // The newer segment still carries its pointer.
        let new_s4_decoded = decode(&repo, &new_s4.to_string());
        assert!(
            matches!(new_s4_decoded.segment_prev, Some(SegmentPrev::At(_))),
            "segment 2 still has an At pointer"
        );

        let repo = fx.repo();
        let ids = vec![c1.clone(), c2.clone()];
        let fast = segment_anchors(&repo, &ids).unwrap();
        let slow = full_sweep(&repo, &ids);
        assert_eq!(fast, slow, "must agree even with a pointerless prefix");
        assert_eq!(fast.get(&c1), Some(&new_s2.to_string()));
        assert_eq!(fast.get(&c2), Some(&new_s4.to_string()));
    }
}
