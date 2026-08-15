//! Session spans: the reading half of the capture-side session stamping
//! (`crate::snapshot::message::session_of`). A span is a contiguous run of
//! same-named snapshots on the live chain, newest to oldest; the same name
//! reappearing after a gap is a second, separate span — the work in between
//! was not part of it.
//!
//! `spans` reuses `evolog::evolog` for the chain walk itself (order, ids,
//! times, base/prev linkage) and adds only bounded, per-row message reads to
//! find each snapshot's session — the same "affordable per displayed row"
//! cost class `evolog::fill_short_ids` already spends, not a second walk of
//! the chain.

use crate::error::{Error, Result};
use crate::evolog::{self, EvologOptions};

/// One contiguous run of same-named snapshots on a chain.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SessionSpan {
    pub name: String,
    /// Newest snapshot in the span (hex).
    pub newest: String,
    /// Oldest snapshot in the span (hex).
    pub oldest: String,
    pub snapshots: usize,
    /// Unix seconds of the oldest and newest snapshot.
    pub started: i64,
    pub ended: i64,
}

/// The session a snapshot commit's message carries, if any. One targeted
/// object read — the id is already known (from a walk or a caller's own
/// resolution), so this never re-walks anything.
pub fn snapshot_session(repo: &gix::Repository, id: &str) -> Result<Option<String>> {
    let oid = gix::ObjectId::from_hex(id.as_bytes()).map_err(Error::repo)?;
    let obj = repo.find_object(oid).map_err(Error::repo)?;
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(commit.message().body.and_then(|body| {
        crate::snapshot::message::session_of(&String::from_utf8_lossy(body.as_ref()))
            .map(str::to_string)
    }))
}

/// Spans on the current branch's live chain, newest first. `limit` bounds
/// the underlying snapshot walk exactly as `EvologOptions::limit` does —
/// `None` is unbounded, so a caller answering a bounded question (`ff
/// session list`) must pass one; `spans` itself has no opinion on the cap.
pub fn spans(repo: &gix::Repository, limit: Option<usize>) -> Result<Vec<SessionSpan>> {
    let rows = evolog::evolog(
        repo,
        &EvologOptions {
            limit,
            ..Default::default()
        },
    )?;

    let mut result: Vec<SessionSpan> = Vec::new();
    let mut current: Option<SessionSpan> = None;

    for row in &rows {
        let session = snapshot_session(repo, &row.id)?;
        match (session, &mut current) {
            (Some(name), Some(span)) if span.name == name => {
                // Still the same run: this row is one step older.
                span.oldest = row.id.clone();
                span.snapshots += 1;
                span.started = row.time;
            }
            (Some(name), _) => {
                if let Some(done) = current.take() {
                    result.push(done);
                }
                current = Some(SessionSpan {
                    name,
                    newest: row.id.clone(),
                    oldest: row.id.clone(),
                    snapshots: 1,
                    started: row.time,
                    ended: row.time,
                });
            }
            (None, _) => {
                // A gap: whatever run was open is done. A later row with
                // the same name starts a second span, not a merge.
                if let Some(done) = current.take() {
                    result.push(done);
                }
            }
        }
    }
    if let Some(done) = current.take() {
        result.push(done);
    }
    Ok(result)
}

/// The tree to diff a span's start against: the tree of the snapshot
/// immediately before the span's oldest snapshot (one step further down the
/// chain), or — when the span opens the chain — that snapshot's base
/// commit's tree. A snapshot with neither (an unborn chain's very first
/// capture) diffs against the empty tree.
pub fn span_start_tree(repo: &gix::Repository, span: &SessionSpan) -> Result<gix::ObjectId> {
    let oldest_oid = gix::ObjectId::from_hex(span.oldest.as_bytes()).map_err(Error::repo)?;
    let decoded = evolog::snap_entry(repo, oldest_oid)?
        .ok_or_else(|| Error::msg("session span's oldest snapshot is not a snapshot commit"))?;

    if let Some(prev_oid) = decoded.next {
        return repo
            .find_commit(prev_oid)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)
            .map(|id| id.detach());
    }

    match decoded.entry.base {
        Some(base_hex) => {
            let base_oid = gix::ObjectId::from_hex(base_hex.as_bytes()).map_err(Error::repo)?;
            repo.find_commit(base_oid)
                .map_err(Error::repo)?
                .tree_id()
                .map_err(Error::repo)
                .map(|id| id.detach())
        }
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}
