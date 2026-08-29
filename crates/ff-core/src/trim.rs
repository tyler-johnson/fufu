//! Retention: drop the oldest suffix of the operation log past the
//! `fufu.keep` cutoff (default 90 days, the tip included).
//!
//! One log means one rebuild. There used to be two — a per-branch snapshot
//! chain and the journal — each with its own walk, its own trash ref, its own
//! parent-relinking rules and its own reflog replay, and the two could
//! disagree about what "90 days" had left behind. The pre-trim tip goes to
//! the chain's own trash ref BEFORE anything moves (trim's one-deep undo), the
//! log is rebuilt oldest-survivor→newest, and then **every** branch pointer's
//! reflog is replayed with the original dates, because `ff restore --at @{n}`
//! and `--at <date>` read exactly those lines. A crash mid-replay leaves a
//! shorter-but-valid log plus the full pre-trim state in trash.
//!
//! What is preserved verbatim is a rule rather than an optimization: an
//! operation's record commit and every pin it took stay exactly where they
//! were written. Only what describes the log's *shape* relinks — the three
//! stated links (`fufu-prev`, `fufu-prev-branch`, `fufu-prev-segment`) and
//! the two leading parent slots — because the shape is the thing that
//! changed. Rewriting a recorded fact would be forgery.

use std::collections::HashMap;

use gix::refs::transaction::PreviousValue;

use crate::error::{Error, Result};
use crate::model::{TrimLog, TrimOrphan, TrimPointer, TrimReport};
use crate::ops::message::{self, SegmentLink};
use crate::ops::record::observe_refs;
use crate::ops::{BRANCH_PREFIX, OpKind, OpRecord, walk};
use crate::refs::{delete_ref, write_ref};
use crate::snapshot::config;

#[derive(Debug, Clone, Default)]
pub struct TrimOptions {
    /// Clock injection for tests; `None` = the wall clock.
    pub now: Option<i64>,
    /// Report only; write nothing.
    pub dry_run: bool,
    /// Also drop the pointers of branches that no longer exist.
    pub gone: bool,
    /// Cutoff override in seconds; `None` = `fufu.keep` (default 90d).
    pub keep_secs: Option<i64>,
}

/// One operation as the rebuild needs it: enough to write it again, and
/// nothing that would let the rebuild change what it says.
struct Entry {
    id: gix::ObjectId,
    time: i64,
    subject: String,
    branch: Option<String>,
    /// The base edge, when the operation carried one as a parent.
    base_parent: Option<gix::ObjectId>,
    /// The record commit and every pin, in their original order.
    other_parents: Vec<gix::ObjectId>,
    prev_on_branch: Option<gix::ObjectId>,
    prev_segment: Option<SegmentLink>,
    commit: gix::objs::Commit,
    message: String,
}

pub fn trim(repo: &gix::Repository, opts: &TrimOptions) -> Result<TrimReport> {
    let now = opts.now.unwrap_or_else(crate::ops::append::wall_clock);
    let keep = match opts.keep_secs {
        Some(secs) => secs,
        None => config::keep_secs(repo)?,
    };
    let cutoff = now - keep;

    let mut report = TrimReport {
        pointers: Vec::new(),
        log: None,
        orphans: Vec::new(),
        dry_run: opts.dry_run,
    };

    let mine = crate::ops::chain_id(repo);
    let outcome = trim_chain(repo, &mine, opts, now, cutoff)?;
    let dropped = outcome.log.as_ref().map(|log| log.dropped).unwrap_or(0);
    report.pointers = outcome.pointers;
    report.log = outcome.log;

    if outcome.rewrote {
        // The pass has released its lock, and the note is an ordinary
        // append that takes the log lock for itself — running it under the
        // pass's guard would deadlock against our own writer. A capture
        // landing in the gap appends onto the replayed tip, which is
        // exactly what it would have done anyway.
        //
        // The trim itself becomes a note — non-pinning by design: pinning
        // trim's own pre-state would defeat retention. The trash ref stays
        // its one-deep undo.
        let table = observe_refs(repo)?;
        let head = crate::head::head_state(repo)?;
        let record = OpRecord::new("trim", format!("trim: dropped {dropped} operation(s)"), now);
        let tree = crate::ops::verb::worktree_or_head(repo)?;
        crate::ops::verb::append_op(
            repo,
            OpKind::Note,
            crate::ops::verb::VerbOp {
                record,
                planned: table,
                tree,
                index_tree: crate::index::tree_from_index(repo)?,
                branch: crate::snapshot::chain::chain_name(&head),
                base: crate::snapshot::chain::base_commit(&head)?,
                session: None,
                pins: &[],
            },
            now,
        )?;
    }

    // A live worktree's chain is left alone because it has a process of its
    // own, and its lock is not this pass's to wait on. An orphan's lock will
    // not be waited on by anyone, which is what makes sweeping it safe.
    for chain in crate::linked::orphan_chains(repo)? {
        if chain == mine {
            continue;
        }
        let outcome = trim_chain(repo, &chain, opts, now, cutoff)?;
        if let Some(log) = outcome.log {
            report.orphans.push(TrimOrphan {
                chain,
                dropped: log.dropped,
                kept: log.kept,
                trash_ref: log.trash_ref,
                deleted: log.deleted,
            });
        }
    }

    Ok(report)
}

/// The result of one chain's retention pass.
struct ChainOutcome {
    log: Option<TrimLog>,
    pointers: Vec<TrimPointer>,
    /// True only when the chain was actually rebuilt — the one case that
    /// earns a note.
    rewrote: bool,
}

/// One chain's retention pass: lock, tip, walk, decide, park to trash,
/// rebuild, pointer replay. It never appends the trim note — a note is an
/// ordinary append on this worktree's chain, which the driver makes for this
/// chain's pass alone once the pass has let go of its lock; an orphan's pass
/// earns none at all.
fn trim_chain(
    repo: &gix::Repository,
    chain: &str,
    opts: &TrimOptions,
    now: i64,
    cutoff: i64,
) -> Result<ChainOutcome> {
    // Trim rewrites the log by deleting the ref and replaying it, so a
    // capture arriving mid-pass would see no log at all and start a second
    // one. The lock closes that window as well as the CAS one — see
    // `ops::lock`. A dry run reads and writes nothing, so it takes nothing.
    let held = if opts.dry_run {
        None
    } else {
        match crate::ops::lock::acquire_chain(repo, chain, crate::ops::lock::Wait::Briefly)? {
            Some(guard) => Some(guard),
            None => {
                return Err(Error::coded(
                    "ref/contended",
                    "another fufu process is writing the operation log",
                    vec![],
                ));
            }
        }
    };

    let ops_ref = crate::ops::ops_ref(chain);
    let ops_trash_ref = crate::ops::ops_trash_ref(chain);

    // Collect pointers first: ref iteration must not overlap ref edits.
    let all_pointers = branch_pointers(repo)?;
    let Some(tip) = crate::refs::ref_target(repo, &ops_ref)? else {
        // No log: the only thing that could exist is a stray pointer, and a
        // pointer with nothing behind it is not this pass's business.
        return Ok(ChainOutcome {
            log: None,
            pointers: Vec::new(),
            rewrote: false,
        });
    };

    let entries = walk_log(repo, tip)?;
    let kept = entries.iter().take_while(|e| e.time >= cutoff).count();
    let dropped = entries.len() - kept;

    // Per-branch counts come out of the one walk. There is no per-branch
    // suffix to drop any more — a branch's operations are interleaved with
    // every other branch's in one chain — so what a branch row reports is its
    // share of what the log kept and dropped.
    let mut per_branch: HashMap<String, (usize, usize)> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let branch = entry.branch.clone().unwrap_or_default();
        let row = per_branch.entry(branch).or_insert((0, 0));
        if i < kept {
            row.0 += 1;
        } else {
            row.1 += 1;
        }
    }

    let trash_ref = (dropped > 0 && !opts.dry_run).then(|| ops_trash_ref.clone());
    let mut log_row = TrimLog {
        dropped,
        kept,
        trash_ref: trash_ref.clone(),
        deleted: kept == 0 && dropped > 0,
    };

    // Only the branches this chain actually carries. A pointer whose
    // operations live on another worktree's chain is that worktree's to
    // rebuild: this pass has no replay for it, so deleting it here would
    // delete it for good. One chain per worktree is what makes the ownership
    // question answerable at all — before that, every pointer belonged to
    // the one log and this filter was the identity.
    let pointers: Vec<(String, String, gix::ObjectId)> = all_pointers
        .into_iter()
        .filter(|(_, branch, _)| per_branch.contains_key(branch))
        .collect();

    // Which branches lose their pointer outright: `--gone` ones, and any
    // whose every operation aged out.
    let mut gone_branches: Vec<String> = Vec::new();
    let mut rows: Vec<TrimPointer> = Vec::new();
    for (ref_name, branch, _) in &pointers {
        let (kept_here, dropped_here) = per_branch.get(branch).copied().unwrap_or((0, 0));
        let gone = branch != crate::snapshot::chain::DETACHED
            && repo
                .try_find_reference(&format!("refs/heads/{branch}"))
                .map_err(Error::repo)?
                .is_none();
        // Existence is reported unconditionally; only deletion of a gone
        // branch's pointer stays behind the `--gone` flag.
        let branch_gone = opts.gone && gone;
        if branch_gone {
            gone_branches.push(branch.clone());
        }
        rows.push(TrimPointer {
            r#ref: ref_name.clone(),
            branch: branch.clone(),
            // A `--gone` branch drops nothing from the log. You cannot excise
            // one branch's operations from the middle of a global chain
            // without rewriting every operation after them, so `--gone` now
            // means what it can honestly mean: the pointer goes, and the
            // operations behind it age out on the same cutoff as everything
            // else.
            dropped: if branch_gone { 0 } else { dropped_here },
            kept: if branch_gone { 0 } else { kept_here },
            trash_ref: trash_ref.clone(),
            deleted: branch_gone || (kept_here == 0 && dropped_here > 0),
            gone,
        });
    }

    if opts.dry_run || (dropped == 0 && gone_branches.is_empty()) {
        log_row.trash_ref = None;
        return Ok(ChainOutcome {
            log: Some(log_row),
            pointers: rows,
            rewrote: false,
        });
    }

    if dropped == 0 {
        // `--gone` alone: nothing left the log, so nothing relinks. Rebuilding
        // anyway would rewrite every operation's sha to say the same thing.
        for (ref_name, branch, pointer) in &pointers {
            if gone_branches.contains(branch) {
                delete_ref(repo, ref_name, *pointer, now)?;
            }
        }
        log_row.trash_ref = None;
        return Ok(ChainOutcome {
            log: Some(log_row),
            pointers: rows,
            rewrote: false,
        });
    }

    {
        // Trash first: the whole pre-trim log stays reachable (and gc-proof)
        // before a single ref moves.
        write_ref(
            repo,
            &ops_trash_ref,
            tip,
            PreviousValue::Any,
            now,
            "trim: pre-trim operation log tip",
        )?;
    }

    if kept == 0 && dropped > 0 {
        delete_ref(repo, &ops_ref, tip, now)?;
        for (ref_name, _, pointer) in &pointers {
            delete_ref(repo, ref_name, *pointer, now)?;
        }
        return Ok(ChainOutcome {
            log: Some(log_row),
            pointers: rows,
            rewrote: false,
        });
    }

    // Rebuild oldest-survivor→newest. Filled in survivor order so that by the
    // time a later survivor's links are resolved, whatever they could
    // possibly point at (always older) is already in here.
    let survivors: Vec<&Entry> = entries[..kept].iter().rev().collect();
    let mut old_to_new: HashMap<gix::ObjectId, gix::ObjectId> = HashMap::new();
    let mut replay: Vec<(gix::ObjectId, i64, String)> = Vec::new();
    let mut per_branch_replay: HashMap<String, Vec<(gix::ObjectId, i64, String)>> = HashMap::new();
    let mut prev_new: Option<gix::ObjectId> = None;
    let mut prev_verb_new: Option<gix::ObjectId> = None;

    for entry in &survivors {
        let mut commit = entry.commit.clone();
        let mut skeleton = message::parse(&entry.message).ok_or_else(|| {
            Error::coded(
                "op/unreadable",
                format!(
                    "{} lost its skeleton between the walk and the rebuild",
                    entry.id
                ),
                vec![],
            )
        })?;

        skeleton.prev = prev_new;
        skeleton.prev_on_branch = entry
            .prev_on_branch
            .and_then(|old| old_to_new.get(&old).copied());
        // A segment whose target aged out is now the first one on the log,
        // which is a positive claim (`ChainStart`) rather than an absence: an
        // absent trailer means "written before the link existed" and would
        // send the anchor walk down the slow path forever.
        skeleton.prev_segment = Some(match entry.prev_segment {
            Some(SegmentLink::At(old)) => match old_to_new.get(&old) {
                Some(new) => SegmentLink::At(*new),
                None => SegmentLink::ChainStart,
            },
            _ => SegmentLink::ChainStart,
        });
        // The verb link comes out of the replay rather than the old trailer,
        // because the replay already knows the answer: it runs oldest-first,
        // so the last non-capture written *is* what this op hops to. That
        // also relinks a log written before the trailer existed, which the
        // remap above cannot do — there is nothing to remap.
        skeleton.prev_verb = Some(match prev_verb_new {
            Some(id) => SegmentLink::At(id),
            None => SegmentLink::ChainStart,
        });
        let is_capture = skeleton.kind == OpKind::Capture;

        // Slot 1 is the chain's, and the base rides at slot 2 behind it — so
        // the survivor that becomes the new root loses its base *parent* along
        // with the prev it no longer has. Keeping it would put a user commit
        // at slot 1 and send `git log --first-parent` on the chain walking
        // out through the user's history, which is the shape this whole log
        // exists to avoid; the trailer still records what the base was.
        // Everything after slot 2 — the record commit, every pin — is
        // preserved verbatim, because rewriting a recorded fact is forgery.
        let mut parents: Vec<gix::ObjectId> = Vec::new();
        if let Some(p) = prev_new {
            parents.push(p);
            parents.extend(entry.base_parent);
        }
        parents.extend(entry.other_parents.iter().copied());
        commit.parents = parents.into();
        commit.message = message::rebuild(&entry.message, &skeleton).into();

        let new_id = repo.write_object(&commit).map_err(Error::repo)?.detach();
        old_to_new.insert(entry.id, new_id);
        prev_new = Some(new_id);
        if !is_capture {
            prev_verb_new = Some(new_id);
        }
        replay.push((new_id, entry.time, entry.subject.clone()));
        if let Some(branch) = &entry.branch {
            per_branch_replay.entry(branch.clone()).or_default().push((
                new_id,
                entry.time,
                entry.subject.clone(),
            ));
        }
    }

    // Delete each ref (its reflog goes with it), then replay one single-ref
    // transaction per survivor with the original dates.
    delete_ref(repo, &ops_ref, tip, now)?;
    replay_ref(repo, &ops_ref, &replay)?;

    for (ref_name, branch, pointer) in &pointers {
        delete_ref(repo, ref_name, *pointer, now)?;
        if gone_branches.contains(branch) {
            continue;
        }
        if let Some(lines) = per_branch_replay.get(branch) {
            replay_ref(repo, ref_name, lines)?;
        }
    }

    // The rewrite is over, and the note is an ordinary append that takes the
    // log lock for itself — so let go of it here rather than deadlocking
    // against our own writer. A capture landing in the gap appends onto the
    // replayed tip, which is exactly what it would have done anyway.
    drop(held);

    log_row.trash_ref = trash_ref;
    Ok(ChainOutcome {
        log: Some(log_row),
        pointers: rows,
        rewrote: true,
    })
}

/// Replay a ref's history one single-ref transaction at a time, oldest first,
/// stamping each line with the operation's own date.
fn replay_ref(
    repo: &gix::Repository,
    name: &str,
    lines: &[(gix::ObjectId, i64, String)],
) -> Result<()> {
    let mut expected = PreviousValue::MustNotExist;
    for (id, time, subject) in lines {
        write_ref(repo, name, *id, expected.clone(), *time, subject)?;
        expected = PreviousValue::MustExistAndMatch(gix::refs::Target::Object(*id));
    }
    Ok(())
}

/// Every branch pointer under `refs/fufu/snap/`, as (full ref, branch, tip).
fn branch_pointers(repo: &gix::Repository) -> Result<Vec<(String, String, gix::ObjectId)>> {
    let mut out = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed(BRANCH_PREFIX).map_err(Error::repo)?;
    for reference in iter {
        let reference = reference.map_err(|err| {
            Error::coded(
                "op/unreadable",
                format!("ref iteration failed: {err}"),
                vec![],
            )
        })?;
        let name = reference.name().as_bstr().to_string();
        let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
            continue;
        };
        let branch = name
            .strip_prefix(BRANCH_PREFIX)
            .unwrap_or(&name)
            .to_string();
        out.push((name, branch, tip));
    }
    Ok(out)
}

/// The log newest-first, decoded far enough to rebuild. Stops at the first
/// link that does not lead to an operation — a damaged tail is the end of the
/// log as far as retention is concerned.
fn walk_log(repo: &gix::Repository, tip: gix::ObjectId) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut cur = Some(tip);
    while let Some(id) = cur {
        let Ok(op) = walk::decode(repo, id) else {
            break;
        };
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
        let commit: gix::objs::Commit = commit_ref.into();
        drop(obj);
        let message = String::from_utf8_lossy(commit.message.as_ref()).into_owned();
        let prev = op.prev().map(|p| p.object_id());
        // Read the leading slots positionally, each confirmed against the
        // trailer that claims it — the same rule `walk::decode` uses, so a
        // hand-edited commit degrades to "everything is a pin" here too rather
        // than to a chain link being silently deleted.
        let mut at = 0usize;
        if prev.is_some() && commit.parents.first().copied() == prev {
            at = 1;
        }
        let base = op.base().map(|b| b.object_id());
        let base_parent = match base {
            Some(b) if commit.parents.get(at).copied() == Some(b) => {
                at += 1;
                Some(b)
            }
            _ => None,
        };
        let other_parents: Vec<gix::ObjectId> = commit.parents.iter().copied().skip(at).collect();
        out.push(Entry {
            id,
            time: op.time(),
            subject: op.summary().to_string(),
            branch: op.branch().map(str::to_string),
            base_parent,
            other_parents,
            prev_on_branch: op.prev_on_branch().map(|p| p.object_id()),
            prev_segment: op.prev_segment(),
            commit,
            message,
        });
        cur = prev;
    }
    Ok(out)
}
