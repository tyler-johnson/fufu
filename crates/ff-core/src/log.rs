use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::error::{Error, Result};
use crate::model::{HeadState, LogEntry};

#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// Maximum number of entries; `None` is unlimited.
    pub limit: Option<usize>,
}

/// A lazy walk of the commit history from HEAD, newest commit time first.
/// An unborn HEAD yields an empty iterator — that is a fact, not an error.
pub fn log<'repo>(
    repo: &'repo mut gix::Repository,
    opts: &LogOptions,
) -> Result<Box<dyn Iterator<Item = Result<LogEntry>> + 'repo>> {
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    let repo: &gix::Repository = repo;

    let commit_hex = match crate::head::head_state(repo)? {
        HeadState::Unborn { .. } => return Ok(Box::new(std::iter::empty())),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => commit,
    };
    let head_id = gix::ObjectId::from_hex(commit_hex.as_bytes()).map_err(Error::repo)?;

    let walk = repo
        .rev_walk(Some(head_id))
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(Error::repo)?;

    let entries = walk.map(|info| -> Result<LogEntry> {
        let info = info.map_err(Error::repo)?;
        entry_for(repo, info.id)
    });

    Ok(match opts.limit {
        Some(n) => Box::new(entries.take(n)),
        None => Box::new(entries),
    })
}

/// One commit formatted as a log entry — shared by the plain log walk and the
/// timeline's base rows so both render identically.
pub(crate) fn entry_for(repo: &gix::Repository, id: gix::ObjectId) -> Result<LogEntry> {
    use gix::prelude::ObjectIdExt;
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let short_id = id.attach(repo).shorten().map_err(Error::repo)?.to_string();
    let author = commit.author().map_err(Error::repo)?;
    // Author time, like `git log %at` (log order is by commit time elsewhere).
    let time = author.time().map_err(Error::repo)?.seconds;
    let subject = commit.message().map_err(Error::repo)?.summary().to_string();
    Ok(LogEntry {
        id: id.to_string(),
        short_id,
        subject,
        author_name: author.name.to_string(),
        author_email: author.email.to_string(),
        time,
    })
}
