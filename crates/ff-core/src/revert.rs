//! `ff op revert` and `ff op abandon` — the two `ff op` verbs that are not a
//! move along the log.
//!
//! Revert is the opposite half of `ff op restore`, and the only verb in the
//! family that *writes* an operation: rewinding to a moment records nothing
//! because navigation is not work, but inverting one change while later work
//! stands is itself a thing that happened, and the log should say so.
//!
//! It inverts *refs*, not files. An operation's ref transitions are what it
//! did; its tree is where the working copy stood while it did it. Putting the
//! tree back too would be `ff op restore` with extra steps, and it would
//! silently discard everything done since — the precise outcome revert exists
//! to avoid.

use crate::error::{Error, Result};
use crate::model::RevertReport;
use crate::ops::{OpKind, OpLog, RefTransition, retire, verb};
use crate::refs;
use crate::snapshot::Provenance;

#[derive(Debug, Clone, Default)]
pub struct OpVerbOptions {
    /// Clock injection for tests.
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// Invert one operation's ref transitions, leaving everything after it
/// standing.
pub fn revert(
    repo: &gix::Repository,
    spec: &str,
    opts: &OpVerbOptions,
    prov: &Provenance,
) -> Result<(RevertReport, verb::VerbContext)> {
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress: finish or abort it with git before reverting"),
            vec![],
        ));
    }
    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;
    let log = OpLog::open(repo)?;
    let id = log.resolve(spec)?;
    let target = log.live(id)?;

    // A capture changes no ref by invariant and a note marks rather than does,
    // so neither has anything to invert. The worktree question a capture
    // answers has its own verb, and it is not this one.
    if !matches!(target.kind(), OpKind::Op | OpKind::Foreign) {
        let why = match target.kind() {
            OpKind::Capture => "a capture changes no ref, so there is nothing in it to invert",
            _ => "a note marks something that happened rather than something that was done",
        };
        return Err(Error::coded(
            "undo/not-undoable",
            format!("{id} is a {}: {why}", target.kind().as_str()),
            vec!["ff op log".into(), "ff restore --at-op <op>".into()],
        ));
    }

    let record = target.record()?.cloned().ok_or_else(|| {
        Error::coded(
            "op/unreadable",
            format!("{id} records no ref transitions; there is nothing to invert"),
            vec!["ff op log".into()],
        )
    })?;

    // The inversion, checked against the present before a byte moves. A ref
    // whose value is no longer what the operation left is a ref later work has
    // touched, and inverting it there would silently take that work with it.
    let observed = crate::ops::record::observe_refs(repo)?;
    let mut inverse: Vec<RefTransition> = Vec::new();
    let mut held: Vec<String> = Vec::new();
    for t in &record.refs {
        let current = observed.refs.get(&t.name);
        if current != t.new.as_ref() {
            held.push(format!(
                "{}: the operation left it at {}, and it now stands at {}",
                t.name,
                t.new.as_deref().unwrap_or("(deleted)"),
                current.map(String::as_str).unwrap_or("(deleted)")
            ));
            continue;
        }
        inverse.push(RefTransition {
            name: t.name.clone(),
            old: t.new.clone(),
            new: t.old.clone(),
        });
    }
    if !held.is_empty() {
        return Err(Error::coded(
            "held/op-revert",
            format!(
                "inverting {id} conflicts with work done since; nothing was changed: {}",
                held.join("; ")
            ),
            vec![
                "ff op show <op>".into(),
                "ff op restore <op>".into(),
                "ff op log".into(),
            ],
        ));
    }
    if inverse.is_empty() {
        return Err(Error::coded(
            "op/unreadable",
            format!("{id} moved no refs; there is nothing to invert"),
            vec!["ff op log".into(), "ff op show <op>".into()],
        ));
    }

    // The planned end state: the present with the inversion applied. HEAD is
    // deliberately left where it is — revert undoes a change, not a journey.
    let mut planned = observed.clone();
    for t in &inverse {
        match &t.new {
            Some(sha) => {
                planned.refs.insert(t.name.clone(), sha.clone());
            }
            None => {
                planned.refs.remove(&t.name);
            }
        }
    }

    let mut out = crate::ops::OpRecord::new(
        "op revert",
        format!("revert {} ({})", id.short(8), target.summary()),
        now,
    );
    out.argv = opts.argv.clone();
    out.refs = inverse.clone();
    out.undo_of = Some(id.to_string());
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    for t in &inverse {
        for sha in [&t.old, &t.new].into_iter().flatten() {
            if let Ok(oid) = gix::ObjectId::from_hex(sha.as_bytes()) {
                pins.push(oid);
            }
        }
    }

    let head = crate::head::head_state(repo)?;
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record: out,
            planned,
            // The worktree does not move: the change stops being applied to
            // the refs, and the files on disk are whatever they already were.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: crate::snapshot::chain::chain_name(&head),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    let mut edits = Vec::new();
    for t in &inverse {
        let expected = match &t.old {
            Some(old) => {
                gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                    gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?,
                ))
            }
            None => gix::refs::transaction::PreviousValue::MustNotExist,
        };
        match &t.new {
            Some(new) => {
                let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
                edits.push(refs::update_edit(
                    &t.name,
                    new_id,
                    expected,
                    &format!("fufu: revert of {id}"),
                )?);
            }
            None => {
                let old = t.old.as_ref().expect("a transition with neither side");
                let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
                edits.push(refs::delete_edit(&t.name, old_id)?);
            }
        }
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while reverting; the revert is on the log but did not apply — \
                 re-run it",
                vec![],
            ));
        }
    }

    Ok((
        RevertReport {
            reverted: id.to_string(),
            reverted_summary: target.summary().to_string(),
            refs: inverse,
            pre_op: ctx.pre_op.map(|op| op.to_string()),
        },
        ctx,
    ))
}

/// Retire the branch of the log an operation sits on: it stops being
/// somewhere the log can walk to, and its objects stay exactly where they are.
pub fn abandon(
    repo: &gix::Repository,
    spec: &str,
    opts: &OpVerbOptions,
) -> Result<(String, usize)> {
    let now = crate::ops::verb::now_or_wall_clock(opts.now);
    let log = OpLog::open(repo)?;
    let id = log.resolve(spec)?;
    let target = log.get(id)?;

    // Abandoning what the log is standing on would leave the pointer naming a
    // position nothing resolves. Move off it first — which is a thing the
    // user chooses, not something abandon should do behind their back.
    if log.tip()? == Some(id) {
        return Err(Error::coded(
            "usage/bad-flags",
            format!("{id} is where the log stands: move off it before abandoning it"),
            vec!["ff undo".into(), "ff op restore <op>".into()],
        ));
    }

    let seeds = retire::seeds_reaching(repo, id, target.time())?;
    if seeds.is_empty() {
        return Err(Error::coded(
            "op/not-found",
            format!(
                "{id} is not on a branch of the log the pointer has stood on, so there is \
                 nothing to abandon"
            ),
            vec!["ff op log".into()],
        ));
    }
    let marked = retire::retire(repo, &seeds, now)?;
    crate::ops::index::refresh(repo, crate::ops::index::Kind::Live);
    Ok((id.to_string(), marked))
}
