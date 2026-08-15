//! The write path: one `commit_op` for all four kinds, and the capture that
//! rides it at machine rate.
//!
//! Two refs move together in one transaction — the log tip and the branch's
//! own pointer — so the pointer can never name an op the log has not got, or
//! lag one it has. The journal moved one ref and the snapshot chain moved
//! another, and nothing made the pair atomic; a crash between them left a
//! stale pointer that only a full walk could detect.

use gix::refs::transaction::PreviousValue;

use crate::error::{Error, Result};
use crate::ops::id::OpId;
use crate::ops::message::{self, Skeleton};
use crate::ops::record::{OpRecord, RefsTable};
use crate::ops::walk;
use crate::ops::{BRANCH_PREFIX, OPS_REF, OpKind};
use crate::refs::{self, EditOutcome};
use crate::snapshot::{Provenance, TakeOptions};

/// Verb ops get three attempts at the CAS; a capture gets one, ever.
const MAX_ATTEMPTS: usize = 3;

/// Everything an operation needs before it exists. Deliberately
/// `pub(crate)`: DESIGN's extension rule is that extensions read fufu state
/// and call fufu verbs, and only fufu writes fufu state. A third party gets
/// the whole reader and no way to become a second author of the log.
pub(crate) struct OpDraft {
    pub kind: OpKind,
    /// The commit subject — provenance for a capture, the verb's summary
    /// otherwise.
    pub subject: String,
    /// The worktree at the END of this op. A plan, not an observation.
    pub tree: gix::ObjectId,
    /// The chain the op runs on: a branch name, or `@detached`.
    pub branch: String,
    /// HEAD's commit when the op ran.
    pub base: Option<gix::ObjectId>,
    pub session: Option<String>,
    pub skipped: Vec<String>,
    /// The PLANNED post-op ref table. `None` on a capture, which inherits
    /// its predecessor's blob oid instead.
    pub refs: Option<RefsTable>,
    /// The index at the op, pinned by containment in the record.
    pub index_tree: Option<gix::ObjectId>,
    /// The machine record. Required for every kind but `Capture`.
    pub record: Option<OpRecord>,
    /// Commits the op's ref transitions touch — reachability IS the gc pin.
    pub pins: Vec<gix::ObjectId>,
}

/// What one append attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Append {
    Committed(OpId),
    /// Another fufu process holds the lock or won the race.
    Contended,
}

/// Write one operation and move both refs atomically.
///
/// Contention policy differs by kind, and the difference is the point.
/// A verb op retries the CAS — but only past a *capture*. If the new tip is
/// another verb op, the planned refs table describes a world that has moved,
/// so retrying would write a plan against a state nobody checked; it refuses
/// with `ref/contended` instead. The journal retried blindly into exactly
/// that hole. A capture never retries at all: losing the race means another
/// fufu process on the same worktree already recorded something, and there
/// are a thousand more captures coming.
pub(crate) fn commit_op(repo: &gix::Repository, draft: &OpDraft, now: i64) -> Result<Append> {
    if draft.kind != OpKind::Capture && draft.record.is_none() {
        return Err(Error::coded(
            "op/unreadable",
            format!("a {} operation must carry a record", draft.kind.as_str()),
            vec![],
        ));
    }
    let branch_ref = format!("{BRANCH_PREFIX}{}", draft.branch);
    let attempts = if draft.kind == OpKind::Capture {
        1
    } else {
        MAX_ATTEMPTS
    };

    for attempt in 0..attempts {
        let prev = refs::ref_target(repo, OPS_REF)?;
        let prev_on_branch = refs::ref_target(repo, branch_ref.as_str())?;

        let mut skeleton = Skeleton::new(draft.kind);
        skeleton.branch = Some(draft.branch.clone());
        skeleton.base = draft.base;
        skeleton.prev = prev;
        skeleton.prev_on_branch = prev_on_branch;
        skeleton.session = draft.session.clone();
        skeleton.prev_segment = Some(segment_link(repo, prev_on_branch, draft.base)?);
        skeleton.refs_blob = match &draft.refs {
            Some(table) => Some(
                repo.write_blob(table.to_blob().as_bytes())
                    .map_err(Error::repo)?
                    .detach(),
            ),
            // A capture names its predecessor's table verbatim: the same
            // blob, the same oid, no write. Observing one here would let a
            // hook capture after a foreign `git checkout` refresh the
            // last-seen table and erase the move from the log for good.
            None => match prev {
                None => None,
                Some(prev) => walk::decode(repo, prev)
                    .ok()
                    .and_then(|op| op.refs_blob_oid()),
            },
        };

        let record_id = match &draft.record {
            None => None,
            Some(record) => Some(write_record(repo, record, &skeleton, draft, now)?),
        };

        let mut parents: Vec<gix::ObjectId> = Vec::new();
        parents.extend(prev);
        parents.extend(draft.base);
        parents.extend(record_id);
        for pin in &draft.pins {
            if let Some(commit) = peel_to_commit(repo, *pin)
                && !parents.contains(&commit)
            {
                parents.push(commit);
            }
        }

        let msg = message::build(&draft.subject, &draft.skipped, &skeleton);
        let commit_id = write_commit(repo, draft.tree, parents, &msg, now)?;

        let reflog = message::clean_subject(&draft.subject, message::MAX_SUBJECT);
        let edits = [
            refs::update_edit(OPS_REF, commit_id, expect(prev), &reflog)?,
            refs::update_edit(
                branch_ref.as_str(),
                commit_id,
                expect(prev_on_branch),
                &reflog,
            )?,
        ];
        match refs::commit_edits(repo, edits, now)? {
            EditOutcome::Applied => {
                // The id index rides the append, after the CAS, best effort
                // and silent: a record can never describe an op that is not
                // on the ref, and a derived cache must not hand the write
                // path a new failure mode.
                crate::ops::index::record(repo, prev, commit_id);
                return Ok(Append::Committed(OpId::new(commit_id)));
            }
            EditOutcome::Contended => {
                if draft.kind == OpKind::Capture || attempt + 1 == attempts {
                    break;
                }
                if !retryable(repo, prev)? {
                    break;
                }
            }
        }
    }

    if draft.kind == OpKind::Capture {
        return Ok(Append::Contended);
    }
    Err(Error::coded(
        "ref/contended",
        "the operation log is contended: another fufu operation is in progress",
        vec![],
    ))
}

/// Whether a lost CAS is worth another attempt: the tip is unchanged (we
/// lost a lock, not a race) or the op that beat us is a capture, which
/// changed no ref and so cannot have invalidated our plan.
pub(crate) fn retryable(
    repo: &gix::Repository,
    cas_against: Option<gix::ObjectId>,
) -> Result<bool> {
    let tip = refs::ref_target(repo, OPS_REF)?;
    if tip == cas_against {
        return Ok(true);
    }
    Ok(match tip {
        None => false,
        Some(tip) => walk::decode(repo, tip).is_ok_and(|op| op.is_capture()),
    })
}

fn expect(prev: Option<gix::ObjectId>) -> PreviousValue {
    match prev {
        Some(p) => PreviousValue::MustExistAndMatch(gix::refs::Target::Object(p)),
        None => PreviousValue::MustNotExist,
    }
}

/// The segment skip-link, by the same rule the capture chain has used since
/// segments existed: still on the predecessor's base means still inside its
/// segment, so copy its pointer verbatim; a different base opens a fresh
/// segment pointing at the predecessor itself. One extra object decode here
/// so the display-side walk never pays it per row.
fn segment_link(
    repo: &gix::Repository,
    prev_on_branch: Option<gix::ObjectId>,
    base: Option<gix::ObjectId>,
) -> Result<message::SegmentLink> {
    let Some(prev) = prev_on_branch else {
        return Ok(message::SegmentLink::ChainStart);
    };
    let previous = walk::decode(repo, prev)?;
    Ok(if previous.base().map(|b| b.object_id()) == base {
        previous
            .prev_segment()
            .unwrap_or(message::SegmentLink::At(prev))
    } else {
        message::SegmentLink::At(prev)
    })
}

/// Write the record commit: parentless, so nothing walks *through* it, and
/// so `parents[0].parents.is_empty()` stays a usable tell. Its tree is the
/// whole machine surface of the op — `op.json`, the ref table, the index.
fn write_record(
    repo: &gix::Repository,
    record: &OpRecord,
    skeleton: &Skeleton,
    draft: &OpDraft,
    now: i64,
) -> Result<gix::ObjectId> {
    // The skeleton is written from one set of variables into both places,
    // so `op.json` and the trailers can never disagree about the walk.
    let mut record = record.clone();
    record.branch = skeleton.branch.clone();
    record.base = skeleton.base.map(|id| id.to_string());
    record.prev = skeleton.prev.map(|id| id.to_string());
    record.prev_on_branch = skeleton.prev_on_branch.map(|id| id.to_string());
    record.prev_segment = match skeleton.prev_segment {
        Some(message::SegmentLink::At(id)) => Some(id.to_string()),
        _ => None,
    };
    record.skipped = draft.skipped.clone();

    let op_json = serde_json::to_vec_pretty(&record).map_err(|err| Error::msg(err.to_string()))?;
    let op_blob = repo.write_blob(&op_json).map_err(Error::repo)?.detach();
    let refs_blob = skeleton.refs_blob.ok_or_else(|| {
        Error::coded(
            "op/unreadable",
            "a recording operation must carry a ref table",
            vec![],
        )
    })?;
    let index_tree = draft.index_tree.ok_or_else(|| {
        Error::coded(
            "op/unreadable",
            "a recording operation must carry an index tree",
            vec![],
        )
    })?;

    use gix::objs::tree::{Entry as TreeEntry, EntryKind};
    let tree = gix::objs::Tree {
        entries: vec![
            TreeEntry {
                mode: EntryKind::Tree.into(),
                filename: "index".into(),
                oid: index_tree,
            },
            TreeEntry {
                mode: EntryKind::Blob.into(),
                filename: "op.json".into(),
                oid: op_blob,
            },
            TreeEntry {
                mode: EntryKind::Blob.into(),
                filename: "refs".into(),
                oid: refs_blob,
            },
        ],
    };
    let tree_id = repo.write_object(&tree).map_err(Error::repo)?.detach();
    // No trailers, deliberately: `is_op_commit` reads the skeleton, so a
    // record can never be mistaken for an operation and restored from.
    write_commit(repo, tree_id, Vec::new(), "record\n", now)
}

fn write_commit(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    parents: Vec<gix::ObjectId>,
    message: &str,
    now: i64,
) -> Result<gix::ObjectId> {
    let sig = gix::actor::Signature {
        name: crate::ops::FUFU_NAME.into(),
        email: crate::ops::FUFU_EMAIL.into(),
        time: gix::date::Time {
            seconds: now,
            offset: 0,
        },
    };
    let commit = gix::objs::Commit {
        tree,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };
    Ok(repo.write_object(&commit).map_err(Error::repo)?.detach())
}

/// Pin candidates must be commits: tags peel, everything else drops.
fn peel_to_commit(repo: &gix::Repository, id: gix::ObjectId) -> Option<gix::ObjectId> {
    let obj = repo.find_object(id).ok()?;
    match obj.kind {
        gix::objs::Kind::Commit => Some(id),
        gix::objs::Kind::Tag => obj.peel_to_kind(gix::objs::Kind::Commit).ok().map(|o| o.id),
        _ => None,
    }
}

/// The result of one capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// A new operation was written and both refs moved.
    Created {
        id: OpId,
        /// Files changed against the previous op on this branch.
        changed_files: usize,
        /// Worktree files dropped for exceeding `fufu.maxFileSize`.
        skipped_files: Vec<String>,
        /// Non-fatal problems (e.g. the gc-config write failing).
        warnings: Vec<String>,
    },
    /// The tree is already recorded: nothing to write.
    NoOp { tip: Option<OpId> },
    /// Another capture holds the lock or won the race; this one skips.
    Contended,
}

/// Capture the working tree as an operation.
///
/// Public because capture is fufu's own floor rather than a general "author
/// any operation" surface: it is what `ff hook` drives, and DESIGN's
/// extension contract is that extensions call fufu verbs. What stays
/// `pub(crate)` is [`commit_op`] — the ability to write an op that moves
/// refs, which is the authorship the cache's safety argument depends on.
///
/// Read-only until the two-ref CAS; the index is never written and HEAD is
/// never opened for writing. A crash leaves at worst orphan objects for gc.
pub fn capture(
    repo: &gix::Repository,
    prov: &Provenance,
    opts: &TakeOptions,
) -> Result<CaptureOutcome> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: capture requires a working tree",
            vec![],
        ));
    }
    let head = crate::head::head_state(repo)?;
    let branch = crate::snapshot::chain::chain_name(&head);
    let base = crate::snapshot::chain::base_commit(&head)?;

    // The dedup target is the previous op on THIS branch, never the global
    // tip: after a switch the tip belongs to another branch, and comparing
    // against its tree would make the first capture on arrival look like a
    // change to everything.
    let prev_on_branch = refs::ref_target(repo, &format!("{BRANCH_PREFIX}{branch}"))?;
    let prev_tree = prev_on_branch
        .map(|id| walk::decode(repo, id).map(|op| op.tree()))
        .transpose()?;
    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();

    let scan = crate::snapshot::tree::scan(repo)?;
    let (tree_id, skipped) = if scan.is_empty() {
        // Tier-1: the capture tree IS the head tree — zero object writes.
        match (prev_on_branch, prev_tree) {
            (None, _) => return Ok(CaptureOutcome::NoOp { tip: None }),
            (Some(p), Some(pt)) if pt == head_tree => {
                return Ok(CaptureOutcome::NoOp {
                    tip: Some(OpId::new(p)),
                });
            }
            // The user committed since the last op: record the post-commit
            // state so the timeline stays continuous.
            _ => (head_tree, Vec::new()),
        }
    } else {
        let max = opts
            .max_file_size
            .unwrap_or_else(|| crate::snapshot::config::max_file_size(repo));
        crate::snapshot::tree::assemble(repo, head_tree, &scan, max)?
    };

    // Tier-2: built the tree, but it equals the branch's previous op (or the
    // head's, when the branch has no ops). Orphan blobs above are gc-able.
    let noop_against = prev_tree.unwrap_or(head_tree);
    if tree_id == noop_against {
        return Ok(CaptureOutcome::NoOp {
            tip: prev_on_branch.map(OpId::new),
        });
    }

    let now = opts.now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let changed_files = crate::snapshot::count_file_changes(repo, noop_against, tree_id)?;

    let draft = OpDraft {
        kind: OpKind::Capture,
        subject: prov.subject(),
        tree: tree_id,
        branch,
        base,
        session: prov.session.clone(),
        skipped: skipped.clone(),
        refs: None,
        index_tree: None,
        record: None,
        pins: Vec::new(),
    };
    let id = match crate::ops::OpLog::open(repo)?.append(&draft, now)? {
        Append::Committed(id) => id,
        Append::Contended => return Ok(CaptureOutcome::Contended),
    };

    let mut warnings = Vec::new();
    if prev_on_branch.is_none()
        && let Err(err) = crate::snapshot::config::ensure_gc_config(repo)
    {
        warnings.push(format!("could not write gc config guard: {err}"));
    }
    Ok(CaptureOutcome::Created {
        id,
        changed_files,
        skipped_files: skipped,
        warnings,
    })
}
