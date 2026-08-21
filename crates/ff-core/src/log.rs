//! A path on a log is a question about the tree, not the diff: a commit
//! touches a selector when the entry the selector names in the commit's tree
//! differs from the entry it names in the first parent's tree, and a merge
//! is measured against its first parent only — so a merge that changed the
//! path against the branch it landed on is a row. git's default hides that
//! one: with a pathspec it simplifies history, dropping any merge equal to
//! some parent and following only that parent. fufu measures against the
//! first parent everywhere, which is what `ff show` already prints, and one
//! rule stated plainly beats a simplification nobody can recite. That rule
//! and
//! [`crate::restore::path_selected`]'s agree on what a selector means, and
//! the agreement is stated rather than relied on silently: a directory is
//! itself a tree entry, so `src/` differs exactly when something under it
//! changed — the same set `ff restore src/` writes.
//!
//! Following renames needs the walk topological — `DateOrder`, not
//! commit-time — because following is stateful, and a stateful tracker is
//! only sound when no parent is ever visited before its children. Git's own
//! `--follow` is documented as a poor-man's implementation for exactly this
//! reason.
//!
//! Following applies to a path naming a blob, never a directory: git tracks
//! no such thing as a directory rename. And a revset filters but does not
//! follow: a set has no line of descent to carry a name along, and its
//! members arrive in the set's order, not an ancestry order a tracker could
//! trust.

use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;
use gix::traverse::commit::topo;

use crate::error::{Error, Result};
use crate::model::{ChangeKind, HeadState, LogEntry};
use crate::revset::{Rev, Revset};

#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// Maximum number of commit rows; `None` is unlimited.
    pub limit: Option<usize>,
    /// `-r`: the set the rows come from. `None` is the walk from HEAD, which
    /// is what every caller that predates the revset language means.
    pub revs: Option<Revset>,
    /// Restrict the rows to commits that touch these paths. Empty means every
    /// commit. The rule is [`crate::restore::path_selected`]'s — a file path or
    /// a directory prefix, no globs.
    pub paths: Vec<String>,
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

/// A lazy walk of the commit history, newest first: from HEAD by default, or
/// over `opts.revs` when a revset was given. An unborn HEAD yields an empty
/// iterator — that is a fact, not an error.
pub fn log<'repo>(repo: &'repo mut gix::Repository, opts: &LogOptions) -> Result<Log<'repo>> {
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    let repo: &gix::Repository = repo;

    // A path filter carries tracked names, and those are only sound when no
    // parent is ever visited before its children — a promise only the
    // topological walk makes. A revset never gets it: it filters, and stops.
    let (open, ids) = match &opts.revs {
        None => (
            true,
            if opts.paths.is_empty() {
                head_walk(repo)?
            } else {
                head_topo_walk(repo)?
            },
        ),
        Some(revs) => members(repo, revs)?,
    };

    let ids = path_filter(repo, ids, &opts.paths, opts.revs.is_none());

    // The limit bounds the commit rows and nothing else — `@` is not one of
    // them, so `-n 25` means the same twenty-five commits it has always
    // meant. It sits after the path filter, so `-n 25` is twenty-five
    // matching rows, not twenty-five walked ones. Taking before
    // the map also keeps `-n` honest about cost: rows nobody asked for are
    // never read out of the object database.
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

/// The walk a path filter rides on: from HEAD, newest first, with the
/// topological promise `head_walk` does not carry — no parent is ever
/// visited before its children, which is what the rename tracker needs.
fn head_topo_walk(repo: &gix::Repository) -> Result<Ids<'_>> {
    let commit_hex = match crate::head::head_state(repo)? {
        HeadState::Unborn { .. } => return Ok(Box::new(std::iter::empty())),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => commit,
    };
    let head_id = gix::ObjectId::from_hex(commit_hex.as_bytes()).map_err(Error::repo)?;

    let walk = topo::Builder::from_iters(&repo.objects, Some(head_id), None::<Vec<gix::ObjectId>>)
        .sorting(topo::Sorting::DateOrder)
        .build()
        .map_err(Error::repo)?;

    Ok(Box::new(walk.map(|info| Ok(info.map_err(Error::repo)?.id))))
}

/// The path axis: the ids of the commits that touch any of `paths`, in the
/// order they arrive. An empty `paths` passes the stream through untouched,
/// and `follow` off is the same touch rule with the names never advanced.
fn path_filter<'r>(
    repo: &'r gix::Repository,
    ids: Ids<'r>,
    paths: &[String],
    follow: bool,
) -> Ids<'r> {
    if paths.is_empty() {
        return ids;
    }
    // Normalized once here, the way `path_selected` normalizes its selectors,
    // so `src/` and `src` are one selector for every commit.
    let mut names: Vec<String> = paths
        .iter()
        .map(|path| path.trim_end_matches('/').to_string())
        .collect();
    Box::new(ids.filter_map(move |res| {
        let id = match res {
            Ok(id) => id,
            Err(err) => return Some(Err(err)),
        };
        match touch(repo, id, &mut names, follow) {
            Ok(true) => Some(Ok(id)),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        }
    }))
}

/// One commit against the tracked names: whether it is a row, and — when
/// `follow` is on — which of the names it renamed, each replaced by the name
/// the file wore before this commit.
fn touch(
    repo: &gix::Repository,
    id: gix::ObjectId,
    names: &mut [String],
    follow: bool,
) -> Result<bool> {
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let tree_id = commit.tree_id().map_err(Error::repo)?.detach();
    let tree = commit.tree().map_err(Error::repo)?;
    let first_parent = commit.parent_ids().next().map(|parent| parent.detach());

    let parent = match first_parent {
        Some(parent) => Some(repo.find_commit(parent).map_err(Error::repo)?),
        None => None,
    };
    let parent_tree = match &parent {
        Some(commit) => Some(commit.tree().map_err(Error::repo)?),
        None => None,
    };
    let parent_tree_id = match &parent {
        Some(commit) => Some(commit.tree_id().map_err(Error::repo)?.detach()),
        None => None,
    };

    let mut touched = false;
    for name in names.iter_mut() {
        let this = entry_at(&tree, name)?;
        // `None` means no first parent; `Some(None)` means the path is not
        // in the first parent's tree. The two are not the same fact.
        let in_parent = match &parent_tree {
            Some(tree) => Some(entry_at(tree, name)?),
            None => None,
        };
        touched |=
            this.map(|(oid, _)| oid) != in_parent.and_then(|entry| entry.map(|(oid, _)| oid));

        // The rename boundary: a blob name that is here and absent in the
        // first parent. A root commit has no parent and so no boundary to
        // test, and a directory is never followed.
        if follow
            && matches!(in_parent, Some(None))
            && this.map(|(_, is_tree)| !is_tree) == Some(true)
            && let Some(parent_tree_id) = parent_tree_id
            && let Some(source) = rename_source(repo, parent_tree_id, tree_id, name)?
        {
            *name = source;
        }
    }
    Ok(touched)
}

/// The entry a selector names in a tree — its id, and whether it is a
/// directory — or `None` when the path is not in this tree.
fn entry_at(tree: &gix::Tree<'_>, selector: &str) -> Result<Option<(gix::ObjectId, bool)>> {
    Ok(tree
        .lookup_entry_by_path(selector)
        .map_err(Error::repo)?
        .map(|entry| (entry.id().detach(), entry.mode().is_tree())))
}

/// The name the file at `selector` wore in the parent tree, when this commit
/// is what renamed it; `None` when the commit created the file.
fn rename_source(
    repo: &gix::Repository,
    parent_tree_id: gix::ObjectId,
    tree_id: gix::ObjectId,
    selector: &str,
) -> Result<Option<String>> {
    let diff = crate::changestat::tree_diff(
        repo,
        parent_tree_id,
        tree_id,
        &crate::changestat::DiffOptions::default(),
    )?;
    Ok(diff
        .files
        .iter()
        .find(|stat| {
            stat.path == selector && matches!(stat.kind, ChangeKind::Renamed | ChangeKind::Copied)
        })
        .and_then(|stat| stat.from.clone()))
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
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let short_id = crate::sha::short_oid(id);
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
