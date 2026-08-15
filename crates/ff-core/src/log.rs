use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::error::{Error, Result};
use crate::model::{HeadState, LogEntry};
use crate::revset::{Rev, Revset};

#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// Maximum number of commit rows; `None` is unlimited.
    pub limit: Option<usize>,
    /// `-r`: the set the rows come from. `None` is the walk from HEAD, which
    /// is what every caller that predates the revset language means.
    pub revs: Option<Revset>,
}

/// A log: the commit rows, plus whether the open change belongs to the set
/// they came from.
///
/// The membership flag travels with the rows rather than being recomputed by
/// the caller, because only the set knows. A CLI that asked "is the open
/// change in here?" separately would have to re-evaluate the revset to answer
/// it — a second walk that could disagree with the first if a snapshot landed
/// between them.
pub struct Log<'repo> {
    /// Whether `@` is a member. Always true for the HEAD walk: without `-r`
    /// the open change is the spine's head by definition.
    pub open: bool,
    pub entries: Box<dyn Iterator<Item = Result<LogEntry>> + 'repo>,
}

/// A stream of commit ids: whatever the rows are drawn from, before any of
/// them costs an object read.
type Ids<'r> = Box<dyn Iterator<Item = Result<gix::ObjectId>> + 'r>;

/// A lazy walk of the commit history, newest commit time first: from HEAD by
/// default, or over `opts.revs` when a revset was given. An unborn HEAD yields
/// an empty iterator — that is a fact, not an error.
pub fn log<'repo>(repo: &'repo mut gix::Repository, opts: &LogOptions) -> Result<Log<'repo>> {
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    let repo: &gix::Repository = repo;

    let (open, ids) = match &opts.revs {
        None => (true, head_walk(repo)?),
        Some(revs) => members(repo, revs)?,
    };

    // The limit bounds the commit rows and nothing else — `@` is not one of
    // them, so `-n 25` means the same twenty-five commits it has always meant.
    // Taking before the map also keeps `-n` honest about cost: rows nobody
    // asked for are never read out of the object database.
    let ids: Ids<'repo> = match opts.limit {
        Some(n) => Box::new(ids.take(n)),
        None => ids,
    };

    Ok(Log {
        open,
        entries: Box::new(ids.map(|id| entry_for(repo, id?))),
    })
}

/// Today's walk: from HEAD, newest commit time first.
fn head_walk(repo: &gix::Repository) -> Result<Ids<'_>> {
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

    Ok(Box::new(walk.map(|info| Ok(info.map_err(Error::repo)?.id))))
}

/// The set's members, split into the `@` row and the commit rows.
///
/// Settling membership by peeking one member, rather than by draining the set
/// and looking, is what keeps `-r '::@' -n 2` a two-pop question on a
/// million-commit history. The peek is exact because the open change carries
/// `i64::MAX` as its time, so every plan that can yield it yields it first.
fn members<'r>(repo: &'r gix::Repository, revs: &Revset) -> Result<(bool, Ids<'r>)> {
    let mut members = revs.evaluate(repo)?.peekable();
    let open = matches!(members.peek(), Some(Ok(Rev::Open)));
    Ok((
        open,
        Box::new(members.filter_map(|rev| match rev {
            // The open change has no minted id, so it is never a commit row —
            // it is the `@` row the peek above turned it into.
            Ok(Rev::Open) => None,
            Ok(Rev::Commit(id)) => Some(Ok(id.object_id())),
            Err(err) => Some(Err(err)),
        })),
    ))
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
