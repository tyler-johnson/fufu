//! `ff undo` — whole-repo rollback to the state before an operation.
//!
//! One rule: **undoing operation T restores the complete state recorded by
//! T's parent 1** — its ref table, its tree, its index tree, its HEAD. That is
//! the whole of it, and it is only one rule because every operation records
//! its planned END state on all four axes. The journal recorded a post-op ref
//! table beside a *pre*-op index tree and a pointer to a separate pre-verb
//! snapshot, so undo needed three different lookups and a special case for
//! foreign entries whose index had been observed after the damage. There is
//! one lookup now, and the foreign case is gone with it.
//!
//! Declarative, not selective: everything after the target rolls back with it.
//! Undo records itself first, so undo-of-undo is redo, and re-running after
//! any crash converges — the plan is a state, not a script.

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::model::UndoReport;
use crate::ops::{
    OpId, OpKind, OpLog, OpRecord, Operation, RefTransition, RefsTable, StashEffect, verb,
};
use crate::refs;
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

#[derive(Debug, Clone, Default)]
pub struct UndoOptions {
    /// The operation to undo, as a letters-spelled id or prefix; `None` =
    /// newest undoable.
    pub op: Option<String>,
    /// Proceed even when some of the recorded state is gone (trimmed): the
    /// missing pieces are skipped with warnings instead of refusing.
    pub force: bool,
    /// Clock injection for tests.
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

pub fn undo(
    repo: &gix::Repository,
    opts: &UndoOptions,
    prov: &Provenance,
) -> Result<(UndoReport, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to undo",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress: finish or abort it with git before undoing"),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;
    let log = OpLog::open(repo)?;

    // Resolve the target AFTER reconciliation: bare undo then sees (and can
    // undo) freshly absorbed foreign motion.
    let tip = log.tip()?.ok_or_else(|| {
        Error::coded(
            "undo/nothing",
            "no operations recorded yet: nothing to undo",
            vec!["ff log --ops".into()],
        )
    })?;
    let target_id = match &opts.op {
        Some(spec) => log.resolve(spec)?,
        None => newest_undoable(&log, tip)?,
    };
    let target = log.live(target_id)?;
    if !undoable(target.kind()) {
        return Err(not_undoable(target_id, &target));
    }
    let prev_id = target.prev().ok_or_else(|| {
        Error::coded(
            "op/floor",
            format!("{target_id} is the oldest operation on the log; there is nothing before it to roll back to"),
            vec!["ff log --ops".into()],
        )
    })?;
    let prev = log.get(prev_id)?;

    // Operations being rolled back: tip..=target along the log link.
    let mut rolled: Vec<Operation<'_>> = Vec::new();
    let mut cursor = Some(tip);
    loop {
        let Some(id) = cursor else {
            return Err(Error::coded(
                "op/not-found",
                format!("{target_id} is not on the log's chain from the current tip"),
                vec!["ff log --ops".into()],
            ));
        };
        let op = log.get(id)?;
        cursor = op.prev();
        let is_target = op.id() == target_id;
        rolled.push(op);
        if is_target {
            break;
        }
    }

    // The one lookup: everything the predecessor recorded.
    let to_table = prev.refs()?.cloned().ok_or_else(|| {
        Error::coded(
            "op/unreadable",
            format!("{prev_id} records no ref table; there is no state to roll back to"),
            vec!["ff log --ops".into()],
        )
    })?;
    let target_wt_tree = prev.tree();
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

    // A capture carries no record, and therefore no index tree. Undoing *to*
    // one writes a clean index at its HEAD tree, which is more defensible here
    // than the assumption it replaced: fufu does not model the index as
    // user-facing state and writes it only so a foreign `git status` stays
    // honest, and a capture's invariant is that it changed no ref — so the
    // index at that moment is whatever HEAD's tree says it is.
    let index_target = match prev.index_tree()? {
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
                vec!["ff undo --force".into(), "ff config keep <duration>".into()],
            ));
        }
        for m in &missing {
            warnings.push(format!("trimmed, skipped: {m}"));
        }
    }

    // Where the worktree starts: the state this undo's own preamble recorded
    // a moment ago, resolved before any ref moves.
    let from_tree = ctx.pre_tree;
    let target_wt_tree = if missing.iter().any(|m| m.starts_with("recorded worktree")) {
        from_tree // force path: leave the tree alone rather than fail
    } else {
        target_wt_tree
    };

    // Write-ahead: the undo records its plan — which IS the state it is
    // restoring — before touching anything.
    //
    // The count reported is of operations that *did* something. Captures in
    // the range roll back with everything else, but a capture changed no ref
    // by invariant, so counting them would tell the user that undoing one
    // close also undid two other things when it undid one — and the range
    // always contains at least the capture this undo's own preamble took.
    let rolled_count = rolled.iter().filter(|op| !op.is_capture()).count();
    let mut record = OpRecord::new(
        "undo",
        format!(
            "undo {} ({}){}",
            // The short spelling in the subject; `undo_of` below keeps the
            // whole id, which is what a machine reads.
            target_id.short(8),
            target.summary(),
            if rolled_count > 1 {
                format!(" and {} later op(s)", rolled_count - 1)
            } else {
                String::new()
            }
        ),
        now,
    );
    record.argv = opts.argv.clone();
    record.refs = transitions.clone();
    record.head = head_moves.then(|| (observed.head.clone(), to_table.head.clone()));
    record.undo_of = Some(target_id.to_string());
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    for t in &transitions {
        for sha in [&t.old, &t.new].into_iter().flatten() {
            if let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes()) {
                pins.push(id);
            }
        }
    }
    let head = crate::head::head_state(repo)?;
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned: to_table.clone(),
            tree: target_wt_tree,
            index_tree: index_target,
            branch: branch_of_table(&to_table)
                .unwrap_or_else(|| crate::snapshot::chain::chain_name(&head)),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 1. Stash effects, inverted, newest-first: a rolled-back push drops, a
    //    rolled-back drop re-pushes.
    for op in &rolled {
        let Some(op_record) = op.record()? else {
            continue; // a capture performs no stash effect, by invariant
        };
        for effect in op_record.stash.iter().rev() {
            match effect {
                StashEffect::Push { stash: sha, .. } => {
                    let id = gix::ObjectId::from_hex(sha.as_bytes()).map_err(Error::repo)?;
                    match stash::drop_stash_entry(repo, id) {
                        Ok(()) => {}
                        Err(err) => warnings.push(format!("could not drop stash {sha}: {err}")),
                    }
                }
                StashEffect::Drop { stash: sha, branch } => {
                    let id = gix::ObjectId::from_hex(sha.as_bytes()).map_err(Error::repo)?;
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
                    &format!("undo: rollback to before {target_id}"),
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
                    "refs moved while undoing; nothing further was changed — re-run ff undo",
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

    // 6. Pending descriptions, inverted newest-first.
    for op in &rolled {
        if let Some(op_record) = op.record()?
            && let Some(d) = &op_record.description
        {
            let mut meta = branchmeta::read(repo, &d.branch)?;
            meta.pending_description = d.old.clone();
            branchmeta::write(repo, &d.branch, &meta)?;
        }
    }

    let mut files = transition.written;
    files.extend(transition.deleted);
    files.sort();
    files.dedup();
    Ok((
        UndoReport {
            target: target_id.to_string(),
            target_summary: target.summary().to_string(),
            target_kind: target.kind().as_str().to_string(),
            rolled_back: rolled_count,
            refs: transitions,
            head_moved: head_moves.then(|| to_table.head.clone()),
            files,
            warnings,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// Ops and foreign absorptions are undoable; notes and captures are not.
fn undoable(kind: OpKind) -> bool {
    matches!(kind, OpKind::Op | OpKind::Foreign)
}

fn not_undoable(id: OpId, op: &Operation<'_>) -> Error {
    let why = match op.kind() {
        // The whole storage argument rests on a capture changing no ref, and
        // that invariant is exactly what makes undoing one a no-op: there is
        // nothing to put back. Restoring its *tree* is a different verb.
        OpKind::Capture => "a capture changes no ref, so undoing it would change nothing",
        _ => "a note marks something that happened rather than something that was done",
    };
    Error::coded(
        "undo/not-undoable",
        format!("{id} is a {}: {why}", op.kind().as_str()),
        vec!["ff log --ops".into(), "ff restore --at <id>".into()],
    )
}

/// The newest operation worth undoing.
fn newest_undoable(log: &OpLog<'_>, tip: OpId) -> Result<OpId> {
    let mut cursor = Some(tip);
    while let Some(id) = cursor {
        let op = log.get(id)?;
        if undoable(op.kind()) {
            return Ok(id);
        }
        cursor = op.prev();
    }
    Err(Error::coded(
        "undo/nothing",
        "nothing undoable on the log yet",
        vec!["ff log --ops".into()],
    ))
}

fn branch_of_table(table: &RefsTable) -> Option<String> {
    table
        .head
        .strip_prefix("ref:refs/heads/")
        .map(|s| s.to_string())
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
                message: format!("undo: detaching HEAD at {at}").into(),
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
