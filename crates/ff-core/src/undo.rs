//! `ff undo` — whole-repo rollback to the state before a journaled op.
//! Declarative, not selective: the target is the target op's PRE-state
//! (its predecessor entry's ref table, its pre-op index tree, its pre-verb
//! snapshot's worktree), and everything after the target rolls back with
//! it. Undo journals itself first, so undo-of-undo is redo, and re-running
//! after any crash converges — the plan is a state, not a script.

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::journal::{self, Entry, OpKind, OpRecord, RefTransition, StashEffect};
use crate::model::UndoReport;
use crate::refs;
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

#[derive(Debug, Clone, Default)]
pub struct UndoOptions {
    /// Journal-sha prefix of the op to undo; `None` = newest undoable.
    pub op: Option<String>,
    /// Proceed even when some pre-state objects are gone (trimmed): the
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
) -> Result<(UndoReport, journal::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::msg("bare repository: nothing to undo"));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::msg(format!(
            "a {op:?} is in progress: finish or abort it with git before undoing"
        )));
    }

    let ctx = journal::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;

    // Resolve the target AFTER reconciliation: bare undo then sees (and can
    // undo) freshly absorbed foreign motion.
    let tip = journal::tip(repo)?.ok_or_else(|| Error::msg("no journal yet: nothing to undo"))?;
    let target_id = match &opts.op {
        Some(prefix) => journal::resolve_op_prefix(repo, prefix)?,
        None => newest_undoable(repo, tip)?,
    };
    let target = journal::read_entry(repo, target_id)?;
    if target.record.kind == OpKind::Note {
        return Err(Error::msg(format!(
            "{} is a {} note, not an operation; nothing to undo",
            &target_id.to_string()[..8],
            target.record.verb
        )));
    }
    let prev_id = target.prev.ok_or_else(|| {
        Error::msg("that operation is the journal floor; nothing before it to roll back to")
    })?;
    let prev = journal::read_entry(repo, prev_id)?;

    // Entries being rolled back: tip..=target along the prev chain.
    let mut rolled: Vec<Entry> = Vec::new();
    let mut cursor = tip;
    loop {
        let entry = journal::read_entry(repo, cursor)?;
        let entry_prev = entry.prev;
        let is_target = entry.id == target_id;
        rolled.push(entry);
        if is_target {
            break;
        }
        match entry_prev {
            Some(p) => cursor = p,
            None => {
                return Err(Error::msg(format!(
                    "{} is not on the journal's first-parent chain",
                    &target_id.to_string()[..8]
                )));
            }
        }
    }

    let observed = journal::observe_refs(repo)?;
    let to_table = prev.refs.clone();
    let mut warnings: Vec<String> = Vec::new();

    // The declarative diff: what has to move, with CAS expectations from
    // the observed present.
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

    // Trimmed pre-state refusal: every sha we are about to write must exist.
    let mut missing: Vec<String> = Vec::new();
    let check = |sha: &str, what: &str, missing: &mut Vec<String>| {
        if let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes())
            && !matches!(repo.try_find_object(id), Ok(Some(_)))
        {
            missing.push(format!("{what}: {sha}"));
        }
    };
    for t in &transitions {
        if let Some(new) = &t.new {
            check(new, &t.name, &mut missing);
        }
    }
    let wt_tree_source: Option<gix::ObjectId> = match &target.record.pre_snapshot {
        Some(snap) => {
            let id = gix::ObjectId::from_hex(snap.as_bytes()).map_err(Error::repo)?;
            match repo.try_find_object(id) {
                Ok(Some(_)) => Some(
                    repo.find_commit(id)
                        .map_err(Error::repo)?
                        .tree_id()
                        .map_err(Error::repo)?
                        .detach(),
                ),
                _ => {
                    missing.push(format!("pre-op snapshot: {snap}"));
                    None
                }
            }
        }
        None => None,
    };
    // A foreign entry's index tree was captured AFTER the foreign motion —
    // there is no pre-state index on record, so the pre-state HEAD tree
    // stands in (clean-index assumption). Op entries recorded theirs before
    // mutating.
    let index_target = match target.record.kind {
        OpKind::Foreign => head_tree_of_table(repo, &to_table)?,
        _ => target.index_tree,
    };
    if !matches!(repo.try_find_object(index_target), Ok(Some(_))) {
        missing.push(format!("pre-op index tree: {index_target}"));
    }
    if !missing.is_empty() {
        if !opts.force {
            return Err(Error::msg(format!(
                "the pre-op state has been trimmed; cannot restore: {} (ff undo --force rolls back what remains)",
                missing.join(", ")
            )));
        }
        for m in &missing {
            warnings.push(format!("trimmed, skipped: {m}"));
        }
    }

    // Where the worktree lands: the target op's pre-verb snapshot when one
    // exists (fresh or the chain tip that already held the state), else the
    // pre-state HEAD tree — no snapshot means the tree matched HEAD.
    let to_head_tree = head_tree_of_table(repo, &to_table)?;
    let target_wt_tree = wt_tree_source.unwrap_or(to_head_tree);
    // Where it starts: this undo's own pre-verb snapshot (= the tree now),
    // else the current HEAD tree — resolved before any ref moves.
    let from_tree = match &ctx.pre_snapshot {
        Some(snap) => {
            let id = gix::ObjectId::from_hex(snap.as_bytes()).map_err(Error::repo)?;
            repo.find_commit(id)
                .map_err(Error::repo)?
                .tree_id()
                .map_err(Error::repo)?
                .detach()
        }
        None => repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
    };

    // Write-ahead: the undo journals its plan before touching anything.
    let rolled_count = rolled.len();
    let mut record = OpRecord::new(
        OpKind::Op,
        "undo",
        format!(
            "undo {} ({}){}",
            &target_id.to_string()[..8],
            target.record.summary,
            if rolled_count > 1 {
                format!(" and {} later op(s)", rolled_count - 1)
            } else {
                String::new()
            }
        ),
        now,
    );
    record.argv = opts.argv.clone();
    record.branch = branch_of_table(&to_table);
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.refs = transitions.clone();
    record.head = head_moves.then(|| (observed.head.clone(), to_table.head.clone()));
    record.undo_of = Some(target_id.to_string());
    let index_tree_now = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree_now.to_string());
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    for t in &transitions {
        for sha in [&t.old, &t.new].into_iter().flatten() {
            if let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes()) {
                pins.push(id);
            }
        }
    }
    journal::append(repo, &record, &to_table, index_tree_now, &pins, now)?;

    // 1. Stash effects, inverted, newest-first: a rolled-back push drops,
    //    a rolled-back drop re-pushes.
    for entry in &rolled {
        for effect in entry.record.stash.iter().rev() {
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
                    &format!("undo: rollback to pre-{}", &target_id.to_string()[..8]),
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
                return Err(Error::msg(
                    "refs moved while undoing; nothing further was changed — re-run ff undo",
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

    // 4. Worktree, from the state captured a moment ago to the pre-op tree.
    let everything = |_: &str| true;
    let transition = worktree::apply_tree_transition(repo, from_tree, target_wt_tree, &everything)?;

    // 5. Index.
    if matches!(repo.try_find_object(index_target), Ok(Some(_))) {
        crate::index::write_index_for_tree(repo, index_target)?;
    }

    // 6. Pending descriptions, inverted newest-first.
    for entry in &rolled {
        if let Some(d) = &entry.record.description {
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
            target_summary: target.record.summary.clone(),
            target_kind: match target.record.kind {
                OpKind::Op => "op".into(),
                OpKind::Foreign => "foreign".into(),
                OpKind::Note => "note".into(),
            },
            rolled_back: rolled_count,
            refs: transitions,
            head_moved: head_moves.then(|| to_table.head.clone()),
            files,
            warnings,
            pre_snapshot: ctx.pre_snapshot.clone(),
        },
        ctx,
    ))
}

/// The newest entry worth undoing: ops and foreign absorptions, not notes.
fn newest_undoable(repo: &gix::Repository, tip: gix::ObjectId) -> Result<gix::ObjectId> {
    let mut cursor = Some(tip);
    while let Some(id) = cursor {
        let entry = journal::read_entry(repo, id)?;
        if entry.record.kind != OpKind::Note {
            return Ok(id);
        }
        cursor = entry.prev;
    }
    Err(Error::msg("nothing undoable in the journal yet"))
}

fn branch_of_table(table: &journal::RefsTable) -> Option<String> {
    table
        .head
        .strip_prefix("ref:refs/heads/")
        .map(|s| s.to_string())
}

/// The tree of the table's HEAD: branch tip's tree, detached sha's tree, or
/// the empty tree for an unborn branch.
fn head_tree_of_table(repo: &gix::Repository, table: &journal::RefsTable) -> Result<gix::ObjectId> {
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
        refs::EditOutcome::Contended => Err(Error::msg("HEAD is contended")),
    }
}
