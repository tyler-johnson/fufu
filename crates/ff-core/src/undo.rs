//! Moving the repository along the operation log: `ff undo`, `ff redo`, and
//! `ff op restore`.
//!
//! One rule: **landing on operation X restores the complete state X
//! recorded** — its ref table, its tree, its index tree, its HEAD. That is the
//! whole of it, and it is only one rule because every operation records its
//! planned END state on all four axes. The journal recorded a post-op ref
//! table beside a *pre*-op index tree and a pointer to a separate pre-verb
//! snapshot, so undo needed three different lookups and a special case for
//! foreign entries whose index had been observed after the damage. There is
//! one lookup now, and the foreign case is gone with it.
//!
//! Three verbs, one mechanism, and they differ only in how the landing is
//! chosen: `ff undo` steps back one *run*, `ff op restore` names an
//! operation, and `ff redo` reads the ref's own reflog to walk forward again.
//! Implementing them separately is how they drift.
//!
//! **The move is a pointer move, not an append.** `refs/fufu/ops` steps to the
//! landing rather than growing an entry saying that it did, so the log records
//! work and never navigation, and undoing an undo is not something anyone has
//! to reason about. What the pointer steps off stays reachable — the reflog
//! pins it, and `gc.refs/fufu/*.reflogexpire=never` is already set — so
//! "recorded where git already keeps such things" costs no new ref and no
//! trash entry. The log answers what happened; the reflog answers where you
//! have stood.
//!
//! Declarative, not selective: everything between here and the landing moves
//! with it, and re-running after any crash converges — the plan is a state,
//! not a script.

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::model::RewindReport;
use crate::ops::{
    OPS_REF, OpId, OpKind, OpLog, Operation, RefTransition, RefsTable, StashEffect, verb, walk,
};
use crate::refs;
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

/// The reflog messages a pointer move writes, and the whole of what `ff redo`
/// reads to decide whether it may move at all.
///
/// Machine-recognizable on purpose: a merely descriptive message would make
/// "was the last thing that happened navigation or work?" a guess, and the
/// answer decides whether moving forward abandons something. Capture reflog
/// lines carry a provenance subject (`pre: ff commit`), and no verb writes a
/// `fufu:`-prefixed line by any other route, so neither prefix can collide.
const UNDO_MOVE: &str = "fufu: undo to ";
const REDO_MOVE: &str = "fufu: redo to ";

/// How the landing is chosen. The three ways in; one way through.
#[derive(Debug, Clone)]
pub enum Landing {
    /// One run back from the tip — `ff undo`, argument-free and repeatable.
    OneRun,
    /// A named operation — `ff op restore <op>`.
    At(String),
    /// Forward along the reflog, reversing the newest undo not already
    /// reversed — `ff redo`.
    Forward,
}

#[derive(Debug, Clone, Default)]
pub struct RewindOptions {
    /// Land on what remains when part of the recorded state is gone
    /// (trimmed): the missing pieces are skipped with warnings instead of
    /// refusing.
    pub force: bool,
    /// Clock injection for tests.
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// `ff undo` — step back one run.
pub fn undo(
    repo: &gix::Repository,
    opts: &RewindOptions,
    prov: &Provenance,
) -> Result<(RewindReport, verb::VerbContext)> {
    rewind(repo, &Landing::OneRun, opts, prov)
}

/// `ff redo` — step forward again.
pub fn redo(
    repo: &gix::Repository,
    opts: &RewindOptions,
    prov: &Provenance,
) -> Result<(RewindReport, verb::VerbContext)> {
    rewind(repo, &Landing::Forward, opts, prov)
}

/// Land the repository on one operation, whichever way that operation was
/// chosen.
pub fn rewind(
    repo: &gix::Repository,
    landing: &Landing,
    opts: &RewindOptions,
    prov: &Provenance,
) -> Result<(RewindReport, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to move",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress: finish or abort it with git before moving"),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;

    // Held from the read of the tip below until the pointer has moved. The
    // CAS on that move is `MustExistAndMatch` like an append's, and carries
    // exactly as little weight — see `ops::lock`. Taken after the preamble's
    // own capture, which takes and releases the same lock.
    let Some(_held) = crate::ops::lock::acquire(repo, crate::ops::lock::Wait::Briefly)? else {
        return Err(Error::coded(
            "ref/contended",
            "another fufu process is writing the operation log",
            vec![],
        ));
    };
    let log = OpLog::open(repo)?;

    // Resolved AFTER reconciliation and after this verb's own capture, which
    // is what puts the pre-move capture at the head of the branch being
    // abandoned — so a later step back hands the held work over first.
    let tip = log.tip()?.ok_or_else(|| {
        Error::coded(
            "undo/nothing",
            "no operations recorded yet: nothing to undo",
            vec!["ff op log".into()],
        )
    })?;

    let (target_id, from_reflog, collapsed) = choose(repo, &log, tip, landing)?;
    // Redo skips the liveness check by construction: its target came off the
    // reflog and was confirmed an operation there, and "on the live walk" is
    // precisely what it is not.
    let target = if opts.force || from_reflog {
        log.get(target_id)?
    } else {
        log.live(target_id)?
    };

    // The two halves of the move: what stops being on the log, and what
    // starts being on it. One of them is empty in the ordinary case — a step
    // back has nothing to enter, a redo has nothing to leave — and both are
    // populated when the landing sits on a branch of the log the pointer
    // forked away from, which is exactly what `ff op restore` on an abandoned
    // id asks for.
    let (back_ids, fwd_ids) = split(&log, tip, target_id)?;
    let back: Vec<Operation<'_>> = back_ids
        .iter()
        .map(|id| log.get(*id))
        .collect::<Result<_>>()?;
    let fwd: Vec<Operation<'_>> = fwd_ids
        .iter()
        .map(|id| log.get(*id))
        .collect::<Result<_>>()?;
    let forward = back.is_empty() && !fwd.is_empty();

    // The one lookup: everything the landing recorded.
    let to_table = target.refs()?.cloned().ok_or_else(|| {
        Error::coded(
            "op/unreadable",
            format!("{target_id} records no ref table; there is no state to move to"),
            vec!["ff op log".into()],
        )
    })?;
    let mut target_wt_tree = target.tree();
    let mut warnings: Vec<String> = Vec::new();

    // The declarative diff: what has to move, with CAS expectations from the
    // observed present.
    let observed = crate::ops::record::observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let names: std::collections::BTreeSet<&String> =
        observed.refs.keys().chain(to_table.refs.keys()).collect();
    for name in names {
        let from = observed.refs.get(name);
        let to = to_table.refs.get(name);
        if from != to {
            transitions.push(RefTransition {
                name: name.to_string(),
                old: from.cloned(),
                new: to.cloned(),
            });
        }
    }
    let head_moves = observed.head != to_table.head;

    // A capture carries no record, and therefore no index tree. Landing *on*
    // one writes a clean index at its HEAD tree, which is more defensible here
    // than the assumption it replaced: fufu does not model the index as
    // user-facing state and writes it only so a foreign `git status` stays
    // honest, and a capture's invariant is that it changed no ref — so the
    // index at that moment is whatever HEAD's tree says it is.
    let index_target = match target.index_tree()? {
        Some(tree) => tree,
        None => head_tree_of_table(repo, &to_table)?,
    };

    // Trimmed state refusal: every object we are about to write must exist.
    let mut missing: Vec<String> = Vec::new();
    let mut check = |id: gix::ObjectId, what: &str| {
        if !matches!(repo.try_find_object(id), Ok(Some(_))) {
            missing.push(format!("{what}: {id}"));
        }
    };
    for t in &transitions {
        if let Some(new) = &t.new
            && let Ok(id) = gix::ObjectId::from_hex(new.as_bytes())
        {
            check(id, &t.name);
        }
    }
    check(target_wt_tree, "recorded worktree");
    check(index_target, "recorded index");
    if !missing.is_empty() {
        if !opts.force {
            return Err(Error::coded(
                "undo/trimmed",
                format!(
                    "the recorded state has been trimmed; cannot restore: {}",
                    missing.join(", ")
                ),
                vec![
                    "ff op restore <op> --force".into(),
                    "ff config keep <duration>".into(),
                ],
            ));
        }
        for m in &missing {
            warnings.push(format!("trimmed, skipped: {m}"));
        }
    }

    // Where the worktree starts: the state this verb's own preamble recorded a
    // moment ago, resolved before any ref moves.
    let from_tree = ctx.pre_tree;
    if missing.iter().any(|m| m.starts_with("recorded worktree")) {
        target_wt_tree = from_tree; // force path: leave the tree alone rather than fail
    }

    // 1. Stash effects, in the order the move makes them true: everything
    //    left behind inverts newest-first (a rolled-back push drops, a
    //    rolled-back drop re-pushes), then everything entered replays
    //    oldest-first. The stash *ref* is in the table like any other, but
    //    its reflog is a stack, so moving the ref alone would leave the
    //    entries behind it disagreeing with it.
    for (op, replay) in replay_order(&back, &fwd) {
        let Some(op_record) = op.record()? else {
            continue; // a capture performs no stash effect, by invariant
        };
        let effects: Vec<&StashEffect> = if replay {
            op_record.stash.iter().collect()
        } else {
            op_record.stash.iter().rev().collect()
        };
        for effect in effects {
            // Entering replays the effect as it was; leaving inverts it.
            let push_it = matches!(
                (effect, replay),
                (StashEffect::Push { .. }, true) | (StashEffect::Drop { .. }, false)
            );
            let (sha, branch) = match effect {
                StashEffect::Push { stash, branch } => (stash, branch),
                StashEffect::Drop { stash, branch } => (stash, branch),
            };
            let id = gix::ObjectId::from_hex(sha.as_bytes()).map_err(Error::repo)?;
            if push_it {
                if !matches!(repo.try_find_object(id), Ok(Some(_))) {
                    warnings.push(format!("stash {sha} is gone; cannot re-push"));
                    continue;
                }
                let expected = match refs::ref_target(repo, stash::STASH_REF)? {
                    Some(tip) => gix::refs::transaction::PreviousValue::MustExistAndMatch(
                        gix::refs::Target::Object(tip),
                    ),
                    None => gix::refs::transaction::PreviousValue::MustNotExist,
                };
                refs::write_ref(
                    repo,
                    stash::STASH_REF,
                    id,
                    expected,
                    now,
                    &format!("On {branch}: fufu: wip on {branch}"),
                )?;
            } else {
                match stash::drop_stash_entry(repo, id) {
                    Ok(()) => {}
                    Err(err) => warnings.push(format!("could not drop stash {sha}: {err}")),
                }
            }
        }
    }

    // 2. Everything else, one atomic transaction. refs/stash was handled
    //    above; CAS expectations come from the observed present, so a crash
    //    and re-run sees the remainder as the new diff.
    let mut edits = Vec::new();
    for t in &transitions {
        if t.name == "refs/stash" {
            continue;
        }
        match (&t.old, &t.new) {
            (_, Some(new)) => {
                let Ok(new_id) = gix::ObjectId::from_hex(new.as_bytes()) else {
                    continue;
                };
                if !matches!(repo.try_find_object(new_id), Ok(Some(_))) {
                    continue; // force path: warned above
                }
                let expected = match &t.old {
                    Some(old) => gix::refs::transaction::PreviousValue::MustExistAndMatch(
                        gix::refs::Target::Object(
                            gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?,
                        ),
                    ),
                    None => gix::refs::transaction::PreviousValue::MustNotExist,
                };
                edits.push(refs::update_edit(
                    &t.name,
                    new_id,
                    expected,
                    &format!("fufu: state as of {target_id}"),
                )?);
            }
            (Some(old), None) => {
                let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
                edits.push(refs::delete_edit(&t.name, old_id)?);
            }
            (None, None) => {}
        }
    }
    if !edits.is_empty() {
        match refs::commit_edits(repo, edits, now)? {
            refs::EditOutcome::Applied => {}
            refs::EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    "refs moved while the repository was being moved; nothing further was \
                     changed — re-run the command",
                    vec![],
                ));
            }
        }
    }

    // 3. HEAD.
    if head_moves {
        match to_table.head.strip_prefix("ref:") {
            Some(target_ref) => crate::branch::retarget_head(repo, target_ref, now)?,
            None => {
                let id = gix::ObjectId::from_hex(to_table.head.as_bytes()).map_err(Error::repo)?;
                detach_head(repo, id, now)?;
            }
        }
    }

    // 4. Worktree, from the state the preamble recorded to the recorded one.
    let everything = |_: &str| true;
    let transition = worktree::apply_tree_transition(repo, from_tree, target_wt_tree, &everything)?;

    // 5. Index.
    if matches!(repo.try_find_object(index_target), Ok(Some(_))) {
        crate::index::write_index_for_tree(repo, index_target)?;
    }

    // 6. Pending descriptions and recorded parents, in the same order and by
    //    the same rule: what is left behind restores its `old`, what is
    //    entered applies its `new`, and the last write is the one the
    //    landing recorded.
    for (op, replay) in replay_order(&back, &fwd) {
        if let Some(op_record) = op.record()? {
            if let Some(d) = &op_record.description {
                let mut meta = branchmeta::read(repo, &d.branch)?;
                meta.pending_description = if replay { d.new.clone() } else { d.old.clone() };
                branchmeta::write(repo, &d.branch, &meta)?;
            }
            if let Some(p) = &op_record.parent {
                let mut meta = branchmeta::read(repo, &p.branch)?;
                meta.parent = if replay { p.new.clone() } else { p.old.clone() };
                branchmeta::write(repo, &p.branch, &meta)?;
            }
        }
    }

    // 7. The pointer itself, last: everything above is idempotent against the
    //    landing, so a crash before this leaves the log naming a state the
    //    world is already in, and re-running converges.
    move_pointer(repo, tip, target_id, &back, &fwd, forward, now)?;

    let mut files = transition.written;
    files.extend(transition.deleted);
    files.sort();
    files.dedup();
    // The newest *decision* the move touched, preferring what was left
    // behind. Both other kinds are skipped for the same reason and it is the
    // reason they are not decisions: a capture only records the working tree,
    // and a note marks something that happened rather than something that was
    // done. So "undid the close" is what a step back over a capture, a close
    // and a trim note reports — which is what actually came back.
    let decision = |op: &&Operation<'_>| matches!(op.kind(), OpKind::Op | OpKind::Foreign);
    let named = back
        .iter()
        .find(decision)
        .or_else(|| fwd.iter().find(decision))
        .or_else(|| back.first())
        .or_else(|| fwd.first());
    Ok((
        RewindReport {
            landed: target_id.to_string(),
            landed_summary: target.summary().to_string(),
            landed_kind: target.kind().as_str().to_string(),
            stepped: back.len() + fwd.len(),
            stepped_ops: back
                .iter()
                .chain(&fwd)
                .filter(|op| !op.is_capture())
                .count(),
            stepped_summary: named.map(|op| op.summary().to_string()),
            stepped_kind: named.map(|op| op.kind().as_str().to_string()),
            collapsed,
            forward,
            refs: transitions,
            head_moved: head_moves.then(|| to_table.head.clone()),
            files,
            warnings,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// Both halves of a move, paired with whether each operation is being
/// *entered* (replay its effects as recorded) or *left* (invert them).
///
/// Leaving happens newest-first and entering oldest-first, so the sequence
/// reads as one continuous walk from where the log stood to where it is
/// going — down one branch and up the other.
fn replay_order<'a, 'r>(
    back: &'a [Operation<'r>],
    fwd: &'a [Operation<'r>],
) -> impl Iterator<Item = (&'a Operation<'r>, bool)> + 'a {
    back.iter()
        .map(|op| (op, false))
        .chain(fwd.iter().rev().map(|op| (op, true)))
}

/// What the move leaves and what it enters, as two lists in newest-first
/// order, meeting at the operation both branches of the log share.
///
/// The general shape is a fork, and the two familiar cases are its
/// degenerate ends: stepping back leaves ops and enters none, redo enters ops
/// and leaves none. Landing on an abandoned id is the case that needs the
/// general form — the pointer forked away from that branch, so getting there
/// means walking down to where the two agree and back up the other side.
///
/// The search runs from both ends at once rather than collecting one whole
/// ancestry first: that costs the distance to the fork in each direction,
/// which for the two ordinary cases is exactly the distance to the answer.
fn split(log: &OpLog<'_>, tip: OpId, target: OpId) -> Result<(Vec<OpId>, Vec<OpId>)> {
    if tip == target {
        return Ok((Vec::new(), Vec::new()));
    }
    let (mut back, mut fwd) = (Vec::new(), Vec::new());
    let mut seen_back: std::collections::HashSet<OpId> = std::collections::HashSet::new();
    let mut seen_fwd: std::collections::HashSet<OpId> = std::collections::HashSet::new();
    let (mut a, mut b) = (Some(tip), Some(target));
    loop {
        if a.is_none() && b.is_none() {
            return Err(Error::coded(
                "op/not-found",
                format!("{target} shares no history with the operation the log stands on"),
                vec!["ff op log".into()],
            ));
        }
        if let Some(id) = a {
            if let Some(cut) = fwd.iter().position(|x| *x == id) {
                fwd.truncate(cut); // the shared operation belongs to neither list
                return Ok((back, fwd));
            }
            back.push(id);
            seen_back.insert(id);
            a = log.get(id)?.prev();
        }
        if let Some(id) = b {
            if seen_back.contains(&id) {
                let cut = back.iter().position(|x| *x == id).expect("just seen");
                back.truncate(cut);
                return Ok((back, fwd));
            }
            fwd.push(id);
            seen_fwd.insert(id);
            b = log.get(id)?.prev();
        }
    }
}

/// Pick the landing. Returns the operation, whether it came off the reflog
/// (and so is deliberately not on the live walk), and how many operations a
/// run collapsed into this one step.
fn choose(
    repo: &gix::Repository,
    log: &OpLog<'_>,
    tip: OpId,
    landing: &Landing,
) -> Result<(OpId, bool, usize)> {
    match landing {
        Landing::At(spec) => Ok((log.resolve(spec)?, false, 0)),
        Landing::OneRun => {
            let run = newest_undoable_run(repo, tip)?;
            let prev = run.prev.ok_or_else(|| {
                Error::coded(
                    "op/floor",
                    format!(
                        "{} is the oldest operation on the log; there is nothing before it to \
                         step back to",
                        run.oldest
                    ),
                    vec!["ff op log".into()],
                )
            })?;
            Ok((prev, false, run.len))
        }
        Landing::Forward => Ok((forward_target(repo, tip)?, true, 0)),
    }
}

/// The newest run worth stepping back over.
///
/// Captures group into a run and a verb operation is always its own, so the
/// only kind skipped here is a note: it marks something that happened rather
/// than something that was done, and there is no state behind it to put back.
fn newest_undoable_run(repo: &gix::Repository, tip: OpId) -> Result<crate::ops::Run> {
    let mut cursor = Some(tip);
    while let Some(id) = cursor {
        let run = walk::run_at(repo, id)?;
        if run.kind != OpKind::Note {
            return Ok(run);
        }
        cursor = run.prev;
    }
    Err(Error::coded(
        "undo/nothing",
        "nothing to undo on the log yet",
        vec!["ff op log".into()],
    ))
}

/// Where `ff redo` goes: the value the ops ref held before the newest undo
/// move that has not already been reversed.
///
/// The reflog is read as a stack rather than as one entry, because a redo
/// writes a line of its own and a second redo must reverse the undo *before*
/// the one the first redo already took — reading only `@{0}` would send it
/// straight back where it came from. Every redo line consumes one undo line;
/// the first undo line left unconsumed is the one to reverse.
///
/// Anything that is neither stops the scan, and that is the whole of the
/// staleness test: work after an undo forks the log rather than truncating
/// it, so redo stops offering a path it can no longer take instead of
/// stepping over something nobody asked it to abandon.
fn forward_target(repo: &gix::Repository, tip: OpId) -> Result<OpId> {
    let nothing = |why: &str| {
        Error::coded(
            "op/nothing-to-redo",
            format!("nothing to redo: {why}"),
            vec!["ff op log".into(), "ff undo".into()],
        )
    };
    let lines = refs::read_ref_log(repo, OPS_REF)?;
    let mut consumed = 0usize;
    for line in lines.iter().rev() {
        if line.message.starts_with(REDO_MOVE) {
            consumed += 1;
            continue;
        }
        if line.message.starts_with(UNDO_MOVE) {
            if consumed > 0 {
                consumed -= 1;
                continue;
            }
            let Some(previous) = line.previous else {
                return Err(nothing("the undo it would reverse has no recorded origin"));
            };
            if !crate::ops::is_op_commit(repo, previous)? {
                return Err(nothing(
                    "the operation it would return to is no longer readable",
                ));
            }
            // Trim is what removes an operation from the resolution domain,
            // and it removes it honestly — the ref goes and its reflog with
            // it. So this is exactly the test for "the way forward has aged
            // out", and it costs one index lookup.
            if !crate::ops::index::contains(repo, crate::ops::index::Kind::Live, previous)? {
                return Err(nothing(
                    "the operations it would return to have been trimmed off the log",
                ));
            }
            let target = OpId::new(previous);
            if target == tip {
                return Err(nothing("the log is already where the undo came from"));
            }
            return Ok(target);
        }
        return Err(nothing(
            "work has landed since the last undo, so the log forked rather than rewound",
        ));
    }
    Err(nothing("no undo has been recorded on this log"))
}

/// Move `refs/fufu/ops` to the landing, and every branch pointer the move
/// invalidated with it, in one transaction.
///
/// Both refs move together for the same reason the append moves them
/// together: a pointer that names an operation the log has not got, or lags
/// one it has, is a state no walk can tell from a crash.
///
/// A branch's pointer is fixed only when the move touched one of its
/// operations, and the fix is exact rather than a search: a branch's touched
/// operations are a contiguous run of its own chain, so what it leaves resets
/// the pointer to what the *oldest* of them named as its predecessor there,
/// and what it enters sets it to the *newest* of them. Entering wins, because
/// it describes where the log is going rather than where it has been.
fn move_pointer(
    repo: &gix::Repository,
    tip: OpId,
    target: OpId,
    back: &[Operation<'_>],
    fwd: &[Operation<'_>],
    forward: bool,
    now: i64,
) -> Result<()> {
    use std::collections::BTreeMap;
    let mut pointers: BTreeMap<String, Option<gix::ObjectId>> = BTreeMap::new();
    // Newest-first, so the last write wins and lands on the oldest.
    for op in back {
        if let Some(branch) = op.branch() {
            pointers.insert(
                branch.to_string(),
                op.prev_on_branch().map(|id| id.object_id()),
            );
        }
    }
    // Newest-first again, and here the *first* is the one to keep — so write
    // oldest-first and let the newest land last.
    for op in fwd.iter().rev() {
        if let Some(branch) = op.branch() {
            pointers.insert(branch.to_string(), Some(op.id().object_id()));
        }
    }

    let message = format!("{}{target}", if forward { REDO_MOVE } else { UNDO_MOVE });
    let mut edits = vec![refs::update_edit(
        OPS_REF,
        target.object_id(),
        gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
            tip.object_id(),
        )),
        &message,
    )?];
    for (branch, value) in pointers {
        let name = format!("{}{branch}", crate::ops::BRANCH_PREFIX);
        let Some(current) = refs::ref_target(repo, &name)? else {
            continue;
        };
        match value {
            Some(id) if id == current => {}
            Some(id) => edits.push(refs::update_edit(
                &name,
                id,
                gix::refs::transaction::PreviousValue::MustExistAndMatch(
                    gix::refs::Target::Object(current),
                ),
                &message,
            )?),
            None => edits.push(refs::delete_edit(&name, current)?),
        }
    }

    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {
            crate::ops::index::refresh(repo, crate::ops::index::Kind::Live);
            Ok(())
        }
        refs::EditOutcome::Contended => Err(Error::coded(
            "ref/contended",
            "the operation log moved while the repository was being moved; the state is \
             already in place — re-run the command to move the pointer",
            vec![],
        )),
    }
}

/// The tree of the table's HEAD: branch tip's tree, detached sha's tree, or
/// the empty tree for an unborn branch.
fn head_tree_of_table(repo: &gix::Repository, table: &RefsTable) -> Result<gix::ObjectId> {
    let commit = match table.head.strip_prefix("ref:") {
        Some(name) => match table.refs.get(name) {
            Some(sha) => Some(gix::ObjectId::from_hex(sha.as_bytes()).map_err(Error::repo)?),
            None => None, // unborn
        },
        None => Some(gix::ObjectId::from_hex(table.head.as_bytes()).map_err(Error::repo)?),
    };
    match commit {
        Some(id) => Ok(repo
            .find_commit(id)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach()),
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// Detach HEAD at a commit (non-dereferencing direct write).
fn detach_head(repo: &gix::Repository, at: gix::ObjectId, now: i64) -> Result<()> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    let edit = RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: format!("fufu: detaching HEAD at {at}").into(),
            },
            expected: PreviousValue::Any,
            new: gix::refs::Target::Object(at),
        },
        name: "HEAD".try_into().map_err(Error::repo)?,
        deref: false,
    };
    match refs::commit_edits(repo, Some(edit), now)? {
        refs::EditOutcome::Applied => Ok(()),
        refs::EditOutcome::Contended => {
            Err(Error::coded("ref/contended", "HEAD is contended", vec![]))
        }
    }
}
