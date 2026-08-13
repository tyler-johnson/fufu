//! Retention: drop the oldest suffix of each chain past the `fufu.keep`
//! cutoff (default 90 days, tips included). The pre-trim tip is saved to
//! `refs/fufu/trash/<branch>` BEFORE anything moves — trim's one-deep undo —
//! then the chain is rebuilt oldest-survivor→newest and its reflog replayed
//! with the original dates, so `@{n}`/`@{time}` stay truthful. A crash
//! mid-replay leaves a shorter-but-valid chain plus the full pre-trim state
//! in trash.

use gix::refs::transaction::PreviousValue;

use crate::error::{Error, Result};
use crate::model::{TrimChain, TrimReport};
use crate::refs::{delete_ref, write_ref};
use crate::snapshot::chain;
use crate::snapshot::config;

#[derive(Debug, Clone, Default)]
pub struct TrimOptions {
    /// Clock injection for tests; `None` = the wall clock.
    pub now: Option<i64>,
    /// Report only; write nothing.
    pub dry_run: bool,
    /// Also drop whole chains whose branch no longer exists.
    pub gone: bool,
    /// Cutoff override in seconds; `None` = `fufu.keep` (default 90d).
    pub keep_secs: Option<i64>,
}

struct ChainEntry {
    id: gix::ObjectId,
    time: i64,
    subject: String,
    /// Original parents with the prev-snapshot slot removed — the base edge,
    /// kept verbatim through any rebuild (rewriting records is forgery).
    base_parents: Vec<gix::ObjectId>,
    has_prev: bool,
}

pub fn trim(repo: &gix::Repository, opts: &TrimOptions) -> Result<TrimReport> {
    let now = opts.now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let keep = match opts.keep_secs {
        Some(secs) => secs,
        None => config::keep_secs(repo)?,
    };
    let cutoff = now - keep;

    // Collect chains first: ref iteration must not overlap ref edits.
    let mut chains: Vec<(String, String, gix::ObjectId)> = Vec::new();
    {
        let platform = repo.references().map_err(Error::repo)?;
        let iter = platform.prefixed(chain::SNAP_PREFIX).map_err(Error::repo)?;
        for reference in iter {
            let reference =
                reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
            let name = reference.name().as_bstr().to_string();
            let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
                continue;
            };
            let branch = name
                .strip_prefix(chain::SNAP_PREFIX)
                .unwrap_or(&name)
                .to_string();
            chains.push((name, branch, tip));
        }
    }

    let mut report = TrimReport {
        chains: Vec::new(),
        journal: None,
        dry_run: opts.dry_run,
    };
    for (ref_name, branch, tip) in chains {
        let entries = walk(repo, tip)?;
        let branch_gone = opts.gone
            && branch != chain::DETACHED
            && repo
                .try_find_reference(&format!("refs/heads/{branch}"))
                .map_err(Error::repo)?
                .is_none();
        let kept = if branch_gone {
            0
        } else {
            entries.iter().take_while(|e| e.time >= cutoff).count()
        };
        let dropped = entries.len() - kept;
        let mut row = TrimChain {
            r#ref: ref_name.clone(),
            branch: branch.clone(),
            dropped,
            kept,
            trash_ref: None,
            deleted: kept == 0 && dropped > 0,
        };
        if dropped == 0 || opts.dry_run {
            report.chains.push(row);
            continue;
        }

        // Trash first: the whole pre-trim chain stays reachable (and
        // gc-proof) before a single ref moves.
        let trash = chain::trash_ref(&branch);
        write_ref(
            repo,
            &trash,
            tip,
            PreviousValue::Any,
            now,
            &format!("trim: pre-trim tip of {branch}"),
        )?;
        row.trash_ref = Some(trash);

        if kept == 0 {
            delete_ref(repo, &ref_name, tip, now)?;
            report.chains.push(row);
            continue;
        }

        // Rebuild oldest-survivor→newest. Trees, messages, and dates are
        // byte-preserved; only parent slots relink, so any commit whose
        // parents don't change reproduces its original sha by content
        // addressing.
        let survivors: Vec<&ChainEntry> = entries[..kept].iter().rev().collect();
        let mut new_tips: Vec<(gix::ObjectId, i64, String)> = Vec::new();
        let mut prev_new: Option<gix::ObjectId> = None;
        for (i, entry) in survivors.iter().enumerate() {
            let obj = repo.find_object(entry.id).map_err(Error::repo)?;
            let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
            let mut commit: gix::objs::Commit = commit_ref.into();
            drop(obj);
            let mut parents: Vec<gix::ObjectId> = Vec::new();
            if i == 0 {
                // The oldest survivor loses its dropped prev; its base edge
                // stands verbatim.
                parents.extend(entry.base_parents.iter().copied());
            } else {
                parents.push(prev_new.expect("set on previous iteration"));
                parents.extend(entry.base_parents.iter().copied());
            }
            // First snapshots never had a prev: nothing relinks, and the
            // written object reproduces the original id.
            if !entry.has_prev && i == 0 {
                parents = entry.base_parents.clone();
            }
            commit.parents = parents.into();
            let new_id = repo.write_object(&commit).map_err(Error::repo)?.detach();
            prev_new = Some(new_id);
            new_tips.push((new_id, entry.time, entry.subject.clone()));
        }

        // Delete the old ref (its reflog goes with it), then replay one
        // single-ref transaction per survivor with the original dates.
        delete_ref(repo, &ref_name, tip, now)?;
        let mut expected = PreviousValue::MustNotExist;
        for (new_id, time, subject) in &new_tips {
            write_ref(repo, &ref_name, *new_id, expected.clone(), *time, subject)?;
            expected = PreviousValue::MustExistAndMatch(gix::refs::Target::Object(*new_id));
        }
        report.chains.push(row);
    }

    report.journal = trim_journal(repo, cutoff, now, opts.dry_run)?;

    // The trim itself becomes a journal note — non-pinning by design:
    // pinning trim's own pre-state would defeat retention. Trash refs stay
    // trim's one-deep undo.
    let trimmed_something = !opts.dry_run
        && (report.chains.iter().any(|c| c.dropped > 0)
            || report.journal.as_ref().is_some_and(|j| j.dropped > 0));
    if trimmed_something && crate::journal::tip(repo)?.is_some() {
        let table = crate::journal::observe_refs(repo)?;
        let snaps: usize = report.chains.iter().map(|c| c.dropped).sum();
        let entries = report.journal.as_ref().map(|j| j.dropped).unwrap_or(0);
        let mut record = crate::journal::OpRecord::new(
            crate::journal::OpKind::Note,
            "trim",
            format!("trim: dropped {snaps} snapshot(s), {entries} journal entr(ies)"),
            now,
        );
        record.branch = None;
        let index_tree = crate::index::tree_from_index(repo)?;
        record.index_tree = Some(index_tree.to_string());
        crate::journal::append(repo, &record, &table, index_tree, &[], now)?;
    }
    Ok(report)
}

/// Journal retention: drop entries past the cutoff, trash-first, rebuilding
/// the surviving chain with parent-1 relinked (pin parents verbatim) and
/// each survivor's `op.json` prev pointer rewritten to match. The reflog is
/// replayed with original times.
fn trim_journal(
    repo: &gix::Repository,
    cutoff: i64,
    now: i64,
    dry_run: bool,
) -> Result<Option<crate::model::TrimJournal>> {
    use crate::journal;
    let Some(tip) = journal::tip(repo)? else {
        return Ok(None);
    };

    // Walk newest-first, collecting decoded entries.
    let mut entries: Vec<journal::Entry> = Vec::new();
    let mut cursor = Some(tip);
    while let Some(id) = cursor {
        let Ok(entry) = journal::read_entry(repo, id) else {
            break; // damaged tail: treat as the end of the chain
        };
        cursor = entry.prev;
        entries.push(entry);
    }

    let kept = entries
        .iter()
        .take_while(|e| e.record.time >= cutoff)
        .count();
    let dropped = entries.len() - kept;
    let mut row = crate::model::TrimJournal {
        dropped,
        kept,
        trash_ref: None,
        deleted: kept == 0 && dropped > 0,
    };
    if dropped == 0 || dry_run {
        return Ok(Some(row));
    }

    // Trash first.
    write_ref(
        repo,
        crate::journal::JOURNAL_TRASH_REF,
        tip,
        PreviousValue::Any,
        now,
        "trim: pre-trim journal tip",
    )?;
    row.trash_ref = Some(crate::journal::JOURNAL_TRASH_REF.to_string());

    if kept == 0 {
        delete_ref(repo, crate::journal::JOURNAL_REF, tip, now)?;
        return Ok(Some(row));
    }

    // Rebuild oldest-survivor→newest. Pin parents (2..n) are preserved
    // verbatim; only the prev link (parent 1 and the op.json field) relinks.
    let survivors: Vec<&journal::Entry> = entries[..kept].iter().rev().collect();
    let mut new_tips: Vec<(gix::ObjectId, i64, String)> = Vec::new();
    let mut prev_new: Option<gix::ObjectId> = None;
    for entry in survivors {
        let obj = repo.find_object(entry.id).map_err(Error::repo)?;
        let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
        let mut commit: gix::objs::Commit = commit_ref.into();
        drop(obj);

        let old_prev = entry.prev;
        let pin_parents: Vec<gix::ObjectId> = commit
            .parents
            .iter()
            .copied()
            .filter(|p| Some(*p) != old_prev)
            .collect();
        let mut parents: Vec<gix::ObjectId> = Vec::new();
        parents.extend(prev_new);
        parents.extend(pin_parents);
        commit.parents = parents.into();

        // Rewrite op.json's prev pointer to the relinked chain.
        let mut record = entry.record.clone();
        record.prev = prev_new.map(|p| p.to_string());
        let op_json =
            serde_json::to_vec_pretty(&record).map_err(|err| Error::msg(err.to_string()))?;
        let op_blob = repo.write_blob(&op_json).map_err(Error::repo)?.detach();
        let tree = repo.find_tree(commit.tree).map_err(Error::repo)?;
        let mut tree_obj: gix::objs::Tree = tree.decode().map_err(Error::repo)?.into();
        for e in &mut tree_obj.entries {
            if e.filename.as_slice() == b"op.json" {
                e.oid = op_blob;
            }
        }
        commit.tree = repo.write_object(&tree_obj).map_err(Error::repo)?.detach();

        let new_id = repo.write_object(&commit).map_err(Error::repo)?.detach();
        prev_new = Some(new_id);
        new_tips.push((new_id, entry.record.time, entry.record.summary.clone()));
    }

    delete_ref(repo, crate::journal::JOURNAL_REF, tip, now)?;
    let mut expected = PreviousValue::MustNotExist;
    for (new_id, time, subject) in &new_tips {
        write_ref(
            repo,
            crate::journal::JOURNAL_REF,
            *new_id,
            expected.clone(),
            *time,
            subject,
        )?;
        expected = PreviousValue::MustExistAndMatch(gix::refs::Target::Object(*new_id));
    }
    Ok(Some(row))
}

/// First-parent walk of a chain, newest first, stopping at the first commit
/// without the fufu identity.
fn walk(repo: &gix::Repository, tip: gix::ObjectId) -> Result<Vec<ChainEntry>> {
    let mut out = Vec::new();
    let mut cur = Some(tip);
    while let Some(id) = cur {
        if !chain::id_is_snapshot(repo, id)? {
            break;
        }
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
        let time = commit.committer.time().map_err(Error::repo)?.seconds;
        let subject = commit.message().summary().to_string();
        let parents: Vec<gix::ObjectId> = commit.parents().collect();
        drop(commit);
        drop(obj);
        let (prev, base_parents) = match parents.split_first() {
            Some((p1, rest)) if chain::id_is_snapshot(repo, *p1)? => (Some(*p1), rest.to_vec()),
            _ => (None, parents.clone()),
        };
        out.push(ChainEntry {
            id,
            time,
            subject,
            base_parents,
            has_prev: prev.is_some(),
        });
        cur = prev;
    }
    Ok(out)
}
