//! Retention: drop the oldest suffix of each chain past the `fufu.keep`
//! cutoff (default 90 days, tips included). The pre-trim tip is saved to
//! `refs/fufu/trash/<branch>` BEFORE anything moves — trim's one-deep undo —
//! then the chain is rebuilt oldest-survivor→newest and its reflog replayed
//! with the original dates, so `@{n}`/`@{time}` stay truthful. A crash
//! mid-replay leaves a shorter-but-valid chain plus the full pre-trim state
//! in trash.

use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::error::{Error, Result};
use crate::model::{TrimChain, TrimReport};
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
    Ok(report)
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

fn committer_ref(time_str: &str) -> gix::actor::SignatureRef<'_> {
    gix::actor::SignatureRef {
        name: chain::FUFU_NAME.into(),
        email: chain::FUFU_EMAIL.into(),
        time: time_str,
    }
}

fn write_ref(
    repo: &gix::Repository,
    name: &str,
    target: gix::ObjectId,
    expected: PreviousValue,
    time: i64,
    message: &str,
) -> Result<()> {
    let time_str = format!("{time} +0000");
    let edit = RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: true,
                message: message.into(),
            },
            expected,
            new: gix::refs::Target::Object(target),
        },
        name: name.try_into().map_err(Error::repo)?,
        deref: false,
    };
    repo.edit_references_as(Some(edit), Some(committer_ref(&time_str)))
        .map_err(|err| Error::msg(format!("could not update {name}: {err}")))?;
    Ok(())
}

fn delete_ref(
    repo: &gix::Repository,
    name: &str,
    expected: gix::ObjectId,
    time: i64,
) -> Result<()> {
    let time_str = format!("{time} +0000");
    let edit = RefEdit {
        change: Change::Delete {
            expected: PreviousValue::MustExistAndMatch(gix::refs::Target::Object(expected)),
            log: RefLog::AndReference,
        },
        name: name.try_into().map_err(Error::repo)?,
        deref: false,
    };
    repo.edit_references_as(Some(edit), Some(committer_ref(&time_str)))
        .map_err(|err| Error::msg(format!("could not delete {name}: {err}")))?;
    Ok(())
}
