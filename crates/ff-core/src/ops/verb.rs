//! The shared preamble of every mutating verb, reconciliation, and the
//! receipt fufu leaves when it takes over a repository that still holds the
//! two old logs.
//!
//! The preamble **reconciles first, then captures**, which is the reverse of
//! what the journal did. Reconcile only reads refs and appends, so it can
//! orphan nothing by running first; capturing first meant that stepping back
//! one operation from a foreign entry landed on a capture that already
//! contained git's damage — the tree had been taken after the outside motion
//! and before anyone noticed it. Reconciling first puts the absorption below
//! the capture, so "the state before this verb" is a state fufu had actually
//! agreed to.

use crate::error::{Error, Result};
use crate::model::{ForeignChange, ReconcileReport};
use crate::ops::append::{self, Append, OpDraft};
use crate::ops::id::OpId;
use crate::ops::record::{OpRecord, RefTransition, RefsTable, observe_refs};
use crate::ops::{BRANCH_PREFIX, OPS_REF, OpKind, OpLog};
use crate::refs;
use crate::snapshot::{Provenance, TakeOptions};

/// Where the two pre-cutover logs are parked. Not a converted history and
/// not a migration — a receipt. See [`park_legacy`].
pub const LEGACY_SNAP_PREFIX: &str = "refs/fufu/legacy/snap/";
pub const LEGACY_TRASH_PREFIX: &str = "refs/fufu/legacy/trash/";
pub const LEGACY_JOURNAL_REF: &str = "refs/fufu/legacy/journal";

/// The old journal's ref, named here only so the receipt can find it. Nothing
/// reads its contents any more.
const OLD_JOURNAL_REF: &str = "refs/fufu/journal";
const OLD_TRASH_PREFIX: &str = "refs/fufu/trash/";

/// The verb preamble's result: the clock, the operation holding the state the
/// verb is about to change, and what reconciliation found on the way in.
#[derive(Debug)]
pub struct VerbContext {
    pub now: i64,
    /// The operation whose tree IS the pre-verb worktree: freshly captured,
    /// or the branch's existing tip when it already held the identical state
    /// (hooks capture constantly, so the no-op case is the common one).
    /// `None` only on a branch with no operations at all.
    pub pre_op: Option<OpId>,
    /// That operation's tree, resolved once here so a verb planning its own
    /// end state never has to decide where "the worktree right now" lives.
    pub pre_tree: gix::ObjectId,
    pub reconcile: ReconcileReport,
}

pub fn now_or_wall_clock(now: Option<i64>) -> i64 {
    now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    })
}

/// Reconcile, then capture. Nothing the verb does afterwards can orphan state
/// that is not already on the log and pinned by it.
pub fn begin_verb(
    repo: &gix::Repository,
    prov: &Provenance,
    now: Option<i64>,
) -> Result<VerbContext> {
    let now = now_or_wall_clock(now);
    let reconcile = reconcile(repo, now)?;
    let capture = crate::ops::capture_with(
        repo,
        prov,
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )?;
    let pre_op = match capture {
        append::CaptureOutcome::Created { id, .. } => Some(id),
        // A no-op with a tip means the branch's newest operation already
        // holds the exact current worktree tree — that op IS the pre-verb
        // state. Dropping it here would make undo of a hook-pre-captured op
        // restore a clean tree instead of the open change.
        append::CaptureOutcome::NoOp { tip, .. } => tip,
        append::CaptureOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "a concurrent fufu capture is in progress; aborted (nothing was written)",
                vec![],
            ));
        }
    };
    let pre_tree = match pre_op {
        Some(id) => OpLog::open(repo)?.get(id)?.tree(),
        None => repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
    };
    Ok(VerbContext {
        now,
        pre_op,
        pre_tree,
        reconcile,
    })
}

/// Everything a mutating verb states about itself before it runs.
///
/// All three of `planned`, `tree` and `index_tree` are the PLANNED END state,
/// and that uniformity is the whole of undo's rule: undoing an operation
/// restores the complete state its predecessor recorded. The journal recorded
/// a post-op ref table beside a *pre*-op index tree, which is why undo needed
/// three separate pre-state lookups and a special case for foreign entries.
pub(crate) struct VerbOp<'p> {
    pub record: OpRecord,
    /// The ref table the world should hold once the op completes.
    pub planned: RefsTable,
    /// The worktree the op plans to leave behind.
    pub tree: gix::ObjectId,
    /// The index the op plans to leave behind.
    pub index_tree: gix::ObjectId,
    /// The chain the op leaves you on — the destination, not the origin, so
    /// the branch pointer that moves is the one the next capture will read.
    pub branch: String,
    /// HEAD's commit when the op ran. Pre-op on purpose: it is the edge that
    /// keeps real history inside the log's ancestry, and the commit a close
    /// *creates* is pinned separately.
    pub base: Option<gix::ObjectId>,
    pub session: Option<String>,
    pub pins: &'p [gix::ObjectId],
}

/// Write one verb operation, write-ahead. Returns its id.
pub(crate) fn append_op(
    repo: &gix::Repository,
    kind: OpKind,
    op: VerbOp<'_>,
    now: i64,
) -> Result<OpId> {
    let draft = OpDraft {
        kind,
        subject: op.record.summary.clone(),
        tree: op.tree,
        branch: op.branch,
        base: op.base,
        session: op.session,
        skipped: Vec::new(),
        refs: Some(op.planned),
        index_tree: Some(op.index_tree),
        record: Some(op.record),
        pins: op.pins.to_vec(),
    };
    match OpLog::open(repo)?.append(&draft, now)? {
        Append::Committed(id) => Ok(id),
        // Only a capture is allowed to shrug contention off; a verb whose
        // plan describes a world that has moved must not write it.
        Append::Contended => Err(Error::coded(
            "ref/contended",
            "the operation log is contended: another fufu operation is in progress",
            vec![],
        )),
    }
}

/// Reconcile the log with reality. Appends at most one operation — `foreign`
/// on divergence, an `init` note on bootstrap. The clean path writes nothing.
pub fn reconcile(repo: &gix::Repository, now: i64) -> Result<ReconcileReport> {
    let observed = observe_refs(repo)?;
    let mut report = ReconcileReport {
        bootstrapped: false,
        reinitialized: false,
        foreign: Vec::new(),
        entry: None,
        warnings: Vec::new(),
    };

    let log = OpLog::open(repo)?;
    let tip = log.tip()?;
    let last_seen: Option<RefsTable> = match tip {
        None => None,
        Some(id) => match log.get(id).and_then(|op| Ok(op.refs()?.cloned())) {
            Ok(table) => table,
            Err(err) => {
                // Unreadable tip: park the whole log, then re-init. The old
                // chain stays reachable (and inspectable) from trash.
                report.reinitialized = true;
                report.warnings.push(format!(
                    "operation log tip unreadable ({err}); log parked at {}",
                    crate::ops::OPS_TRASH_REF
                ));
                refs::write_ref(
                    repo,
                    crate::ops::OPS_TRASH_REF,
                    id.object_id(),
                    gix::refs::transaction::PreviousValue::Any,
                    now,
                    "reconcile: parked unreadable operation log",
                )?;
                refs::delete_ref(repo, OPS_REF, id.object_id(), now)?;
                None
            }
        },
    };

    let Some(last_seen) = last_seen else {
        // Bootstrap: record the observed state as the new floor. Anything
        // before this moment is no longer undoable.
        report.bootstrapped = true;
        report.warnings.extend(park_legacy(repo, now)?);
        let mut record = OpRecord::new(
            "init",
            "operation log initialized from observed state; earlier operations not undoable",
            now,
        );
        record.refs = Vec::new();
        // The floor carries HEAD's tree, not the working tree, and that is the
        // one place the "tree = the worktree at the end of this op" rule bends
        // on purpose. The note means *fufu has observed nothing yet*; claiming
        // the working tree here would make the very first capture a no-op
        // against it and leave `ff` in a fresh repository reporting "no changes
        // since the last snapshot" when there had never been one.
        let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
        let id = append_observed(repo, OpKind::Note, record, &observed, head_tree, now)?;
        report.entry = Some(id.to_string());
        return Ok(report);
    };

    let mut foreign = last_seen.diff(&observed);
    if foreign.is_empty() {
        return Ok(report); // clean: write nothing
    }

    // Divergence. Quote git's own reflog messages as hints, and flag the
    // write-ahead crash case: an op whose planned state never materialized.
    let incomplete = tip
        .and_then(|id| log.get(id).ok())
        .filter(|op| op.kind() == OpKind::Op)
        .and_then(|op| op.record().ok().flatten().cloned())
        .is_some_and(|record| {
            foreign.iter().all(|change| {
                // Reality still holds the op's OLD value: it never ran.
                change.name == "HEAD"
                    || record
                        .refs
                        .iter()
                        .any(|t| t.name == change.name && t.old == change.new)
            })
        });
    for change in &mut foreign {
        change.hint = reflog_hint(repo, &change.name, change.new.as_deref());
    }
    let summary = if incomplete {
        format!(
            "absorbed {} ref change(s); previous op may not have completed",
            foreign.len()
        )
    } else {
        format!("absorbed {} foreign ref change(s)", foreign.len())
    };
    let mut record = OpRecord::new("reconcile", summary, now);
    record.refs = foreign
        .iter()
        .map(|change| RefTransition {
            name: change.name.clone(),
            old: change.old.clone(),
            new: change.new.clone(),
        })
        .collect();
    record.head = head_transition(&last_seen, &observed);
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    for change in &foreign {
        for sha in [&change.old, &change.new].into_iter().flatten() {
            match gix::ObjectId::from_hex(sha.as_bytes()) {
                Ok(id) if repo.try_find_object(id).ok().flatten().is_some() => pins.push(id),
                Ok(_) => report.warnings.push(format!(
                    "{}: {sha} is gone from the object store; not pinnable",
                    change.name
                )),
                Err(_) => {}
            }
        }
    }
    // A foreign op records the present, working tree included: reconciling
    // before capturing is what keeps "one operation back from a foreign
    // absorption" a state fufu agreed to, and a foreign op that claimed HEAD's
    // tree would make undoing to it throw uncommitted work away.
    let tree = worktree_or_head(repo)?;
    let id = append_observed_with_pins(repo, OpKind::Foreign, record, &observed, &pins, tree, now)?;
    report.entry = Some(id.to_string());
    report.foreign = foreign;
    Ok(report)
}

fn append_observed(
    repo: &gix::Repository,
    kind: OpKind,
    record: OpRecord,
    observed: &RefsTable,
    tree: gix::ObjectId,
    now: i64,
) -> Result<OpId> {
    append_observed_with_pins(repo, kind, record, observed, &[], tree, now)
}

/// The assembled working tree, or HEAD's when there is no working tree to
/// assemble. Bare repositories have exactly one tree available to them.
pub(crate) fn worktree_or_head(repo: &gix::Repository) -> Result<gix::ObjectId> {
    if repo.workdir().is_some() {
        Ok(append::worktree_tree(repo, None)?.0)
    } else {
        Ok(repo.head_tree_id_or_empty().map_err(Error::repo)?.detach())
    }
}

/// Append an operation whose "plan" is simply the present: reconciliation
/// records what it found, so the observed worktree, index and refs are the
/// end state by definition rather than by intention.
fn append_observed_with_pins(
    repo: &gix::Repository,
    kind: OpKind,
    record: OpRecord,
    observed: &RefsTable,
    pins: &[gix::ObjectId],
    tree: gix::ObjectId,
    now: i64,
) -> Result<OpId> {
    let head = crate::head::head_state(repo)?;
    let branch = crate::snapshot::chain::chain_name(&head);
    let base = crate::snapshot::chain::base_commit(&head)?;
    append_op(
        repo,
        kind,
        VerbOp {
            record,
            planned: observed.clone(),
            tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch,
            base,
            session: None,
            pins,
        },
        now,
    )
}

/// The receipt.
///
/// `BRANCH_PREFIX` is deliberately the namespace the old capture chains used,
/// because after the cutover it means the same thing — the newest thing fufu
/// wrote on this branch. Which means the first `ff` invocation in a
/// pre-cutover repository would CAS an old chain tip away and orphan every
/// snapshot hanging off it, silently and in one step.
///
/// So before the log is created, every ref that belonged to the two old logs
/// is copied under `refs/fufu/legacy/` and the original deleted. Copying
/// without deleting was the first thing tried and is wrong: the pointer's
/// value would still be a commit the new decoder cannot read, so the very
/// next append would either refuse or write a `fufu-prev-branch` trailer
/// naming a non-operation.
///
/// This is not a converter and not a migration — nothing is rewritten and
/// nothing is read back. "fufu never silently destroys history" is a promise
/// worth keeping true even where the history is being abandoned on purpose.
pub(crate) fn park_legacy(repo: &gix::Repository, now: i64) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let mut parked: Vec<String> = Vec::new();

    for (old_prefix, new_prefix) in [
        (BRANCH_PREFIX, LEGACY_SNAP_PREFIX),
        (OLD_TRASH_PREFIX, LEGACY_TRASH_PREFIX),
    ] {
        // Collect first: ref iteration must not overlap ref edits.
        let mut found: Vec<(String, String, gix::ObjectId)> = Vec::new();
        {
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform.prefixed(old_prefix).map_err(Error::repo)?;
            for reference in iter {
                let reference = reference.map_err(|err| {
                    Error::coded(
                        "op/unreadable",
                        format!("ref iteration failed: {err}"),
                        vec![],
                    )
                })?;
                let name = reference.name().as_bstr().to_string();
                // The log's own trash lives in the trash namespace and is
                // emphatically not legacy: reinitialization writes it moments
                // before this runs, and parking it would move the very thing
                // that was just saved.
                if name == crate::ops::OPS_TRASH_REF {
                    continue;
                }
                let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
                    continue;
                };
                let short = name.strip_prefix(old_prefix).unwrap_or(&name).to_string();
                found.push((name, short, tip));
            }
        }
        for (name, short, tip) in found {
            // An op commit here means a re-init rather than a first run: the
            // log this pointer belongs to is the one reconcile just parked as
            // unreadable, and the pointer goes with it for the same reason.
            let legacy = format!("{new_prefix}{short}");
            refs::write_ref(
                repo,
                &legacy,
                tip,
                gix::refs::transaction::PreviousValue::Any,
                now,
                "cutover: parked pre-cutover chain",
            )?;
            refs::delete_ref(repo, &name, tip, now)?;
            parked.push(legacy);
        }
    }

    if let Some(tip) = refs::ref_target(repo, OLD_JOURNAL_REF)? {
        refs::write_ref(
            repo,
            LEGACY_JOURNAL_REF,
            tip,
            gix::refs::transaction::PreviousValue::Any,
            now,
            "cutover: parked pre-cutover journal",
        )?;
        refs::delete_ref(repo, OLD_JOURNAL_REF, tip, now)?;
        parked.push(LEGACY_JOURNAL_REF.to_string());
    }

    if !parked.is_empty() {
        warnings.push(format!(
            "snapshots and operations from before the one-log cutover are not readable by this \
             fufu; the {} ref(s) holding them were parked under refs/fufu/legacy/ rather than \
             overwritten: {}",
            parked.len(),
            parked.join(", ")
        ));
    }
    Ok(warnings)
}

fn head_transition(old: &RefsTable, new: &RefsTable) -> Option<(String, String)> {
    (old.head != new.head).then(|| (old.head.clone(), new.head.clone()))
}

/// The newest reflog message on `name` whose new value matches `target` —
/// git's own words about what happened, best effort.
fn reflog_hint(repo: &gix::Repository, name: &str, target: Option<&str>) -> Option<String> {
    let target = target?;
    let reference = repo.try_find_reference(name).ok()??;
    let mut platform = reference.log_iter();
    let iter = platform.rev().ok()??;
    for line in iter.flatten() {
        if line.new_oid.to_string() == target {
            let msg = line.message.to_string();
            if !msg.is_empty() {
                return Some(msg);
            }
            break;
        }
    }
    None
}

/// The log as display rows, newest first, captures excluded.
///
/// Captures are the overwhelming majority of the log and say nothing a verb
/// view wants — this is the same set the journal held, read from the one log
/// rather than from a second one.
pub fn read_ops(repo: &gix::Repository, limit: usize) -> Result<Vec<crate::model::OpEntry>> {
    read_ops_from(repo, None, limit, false)
}

/// The same rows, bounded at a past operation and optionally including the
/// capture floor's own.
///
/// `start` is where the walk begins: the tip when `None`, and an operation
/// when `--at-op` or `--at` placed the reader in the past. Bounding rather
/// than filtering is the honest reading of "the log as it stood then" —
/// operations behind a point never change, so the past view is this one with
/// its head cut off.
pub fn read_ops_from(
    repo: &gix::Repository,
    start: Option<OpId>,
    limit: usize,
    captures: bool,
) -> Result<Vec<crate::model::OpEntry>> {
    let log = OpLog::open(repo)?;
    // A verb view hops the runs of captures rather than decoding them to find
    // out it did not want them — twenty-five rows' work whatever the log has
    // grown to. `read_ops_of`'s kind filter stays the guard: the hop only
    // decides what is *walked*, never what is shown.
    let walk: Box<dyn Iterator<Item = Result<crate::ops::Operation<'_>>>> = match (start, captures)
    {
        (Some(id), true) => Box::new(log.iter_from(id)),
        (Some(id), false) => Box::new(log.iter_verbs_from(id)),
        (None, true) => Box::new(log.iter()),
        (None, false) => Box::new(log.iter_verbs()),
    };
    read_ops_of(repo, walk.map(|op| op.map(|op| op.id())), limit, captures)
}

/// The rows for whatever sequence of operations a caller has already chosen,
/// newest first.
///
/// This is where `ff op log <set>` arrives: the set language decides
/// membership and order, and the display layer does not care how. It stays
/// lazy in the caller's iterator, so `ff op log '::@' -n 25` is still
/// twenty-five rows' work at any depth.
pub fn read_ops_of(
    repo: &gix::Repository,
    ids: impl Iterator<Item = Result<OpId>>,
    limit: usize,
    captures: bool,
) -> Result<Vec<crate::model::OpEntry>> {
    let log = OpLog::open(repo)?;
    let mut out = Vec::new();
    let mut hex: Vec<String> = Vec::new();
    for id in ids {
        let Ok(id) = id else {
            break; // damaged history: show what is legible
        };
        let op = log.get(id)?;
        if op.is_capture() && !captures {
            continue;
        }
        if limit != 0 && out.len() >= limit {
            break;
        }
        let record = op.record()?;
        hex.push(op.id().hex());
        out.push(crate::model::OpEntry {
            id: op.id().to_string(),
            // Filled below: abbreviation is priced by the rows on screen.
            short_id: String::new(),
            kind: op.kind().as_str().to_string(),
            verb: record.map(|r| r.verb.clone()).unwrap_or_default(),
            summary: op.summary().to_string(),
            time: op.time(),
            branch: op.branch().map(str::to_string),
            session: op.session().map(str::to_string),
            undo_of: record.and_then(|r| r.undo_of.clone()),
        });
    }
    let lens = crate::ops::index::prefix_lens(repo, &hex)?;
    for (row, hex) in out.iter_mut().zip(&hex) {
        let len = lens.get(hex).copied().unwrap_or(8).max(4);
        row.short_id = row.id.chars().take(len).collect();
    }
    Ok(out)
}

/// Every foreign change a reconcile pass would report, without writing
/// anything — `ff status`'s peek at the same question.
pub fn pending_foreign(repo: &gix::Repository) -> Result<Vec<ForeignChange>> {
    let log = OpLog::open(repo)?;
    let Some(tip) = log.tip()? else {
        return Ok(Vec::new());
    };
    let op = log.get(tip)?;
    let Some(last_seen) = op.refs()? else {
        return Ok(Vec::new());
    };
    Ok(last_seen.diff(&observe_refs(repo)?))
}
