//! Per-file insertions and deletions for the open change.
//!
//! The branch's newest operation carries the worktree as a git tree object, so
//! the diffstat is a plain tree-to-tree diff: HEAD's tree against that tree.
//! Any operation answers, not only a capture — every one of them records the
//! working tree it leaves behind, and asking only captures would report a
//! clean change in a repository whose newest operation happens to be a verb.

use gix::object::tree::diff::{Action, Change};

use crate::error::{Error, Result};
use crate::model::{ChangeKind, ChangeStat, FileStat};

/// Classify the entry mode's type class for type-change detection.
/// 0 = file, 1 = symlink, 2 = commit, 3 = tree.
fn mode_class(mode: gix::object::tree::EntryMode) -> u8 {
    if mode.is_link() {
        1
    } else if mode.is_commit() {
        2
    } else if mode.is_tree() {
        3
    } else {
        0
    }
}

fn classify(change: &Change) -> ChangeKind {
    match change {
        Change::Addition { .. } => ChangeKind::Added,
        Change::Deletion { .. } => ChangeKind::Deleted,
        Change::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } => {
            if mode_class(*previous_entry_mode) != mode_class(*entry_mode) {
                ChangeKind::TypeChange
            } else {
                ChangeKind::Modified
            }
        }
        Change::Rewrite { copy, .. } => {
            if *copy {
                ChangeKind::Copied
            } else {
                ChangeKind::Renamed
            }
        }
    }
}

/// How deep to read the same tree diff, and over which paths.
///
/// One options struct rather than a second entry point per depth: the walk,
/// the classification and the path rule are the same work either way, and
/// only the question of whether to open each blob differs.
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    /// Fill each file's `hunks` — the patch body, not just its size.
    pub hunks: bool,
    /// Restrict the report to these paths. Empty means every file. The rule
    /// is [`crate::restore::path_selected`]'s — a file path or a directory
    /// prefix — so `ff diff src/` selects what `ff restore src/` writes.
    pub paths: Vec<String>,
}

/// The diffstat of the open change. The stat-only spelling of
/// [`change_diff`], kept so the four surfaces that only ever wanted counts
/// do not have to say so.
pub fn change_stat(repo: &gix::Repository) -> Result<ChangeStat> {
    change_diff(repo, &DiffOptions::default())
}

/// The diffstat between two trees, counts only — [`tree_diff`] without a
/// question to ask it.
pub fn tree_diff_stat(
    repo: &gix::Repository,
    old_tree_id: gix::ObjectId,
    new_tree_id: gix::ObjectId,
) -> Result<ChangeStat> {
    tree_diff(repo, old_tree_id, new_tree_id, &DiffOptions::default())
}

/// The mode and blob id each side of a change carries, octal and hex, as
/// git's `diff --git` header spells them. `None` on a side is that side not
/// existing — the `/dev/null` half of an addition or a deletion.
type Sides = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn sides(change: &Change) -> Sides {
    let mode = |m: gix::object::tree::EntryMode| Some(m.kind().as_octal_str().to_string());
    let id = |i: gix::Id<'_>| Some(i.to_string());
    match change {
        Change::Addition {
            entry_mode, id: to, ..
        } => (None, mode(*entry_mode), None, id(*to)),
        Change::Deletion {
            entry_mode,
            id: from,
            ..
        } => (mode(*entry_mode), None, id(*from), None),
        Change::Modification {
            previous_entry_mode,
            previous_id,
            entry_mode,
            id: to,
            ..
        } => (
            mode(*previous_entry_mode),
            mode(*entry_mode),
            id(*previous_id),
            id(*to),
        ),
        Change::Rewrite {
            source_entry_mode,
            source_id,
            entry_mode,
            id: to,
            ..
        } => (
            mode(*source_entry_mode),
            mode(*entry_mode),
            id(*source_id),
            id(*to),
        ),
    }
}

/// Compute the open change: HEAD's tree against the branch's newest
/// operation's tree. Returns an empty result when the branch has no
/// operations, the trees are identical, or the repository is bare.
pub fn change_diff(repo: &gix::Repository, opts: &DiffOptions) -> Result<ChangeStat> {
    // Bare repos: no workdir means no open change to measure.
    if repo.workdir().is_none() {
        return Ok(ChangeStat {
            files: Vec::new(),
            insertions: 0,
            deletions: 0,
        });
    }

    let head = crate::head::head_state(repo)?;
    let branch = crate::snapshot::chain::chain_name(&head);
    let head_tree_id = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();

    let log = crate::ops::OpLog::open(repo)?;
    let Some(tip_id) = log.branch_tip(&branch)? else {
        return Ok(ChangeStat {
            files: Vec::new(),
            insertions: 0,
            deletions: 0,
        });
    };

    tree_diff(repo, head_tree_id, log.get(tip_id)?.tree(), opts)
}

/// The diff between two arbitrary trees. `change_diff` drives this with
/// (HEAD tree, newest operation's tree); `ff op diff` drives it with the trees
/// of two operations, so the tree-diff engine lives in exactly one place
/// regardless of which two trees are in question — and, with
/// [`DiffOptions::hunks`], regardless of how deep the question goes.
pub fn tree_diff(
    repo: &gix::Repository,
    old_tree_id: gix::ObjectId,
    new_tree_id: gix::ObjectId,
    opts: &DiffOptions,
) -> Result<ChangeStat> {
    // Identical trees: nothing to report.
    if old_tree_id == new_tree_id {
        return Ok(ChangeStat {
            files: Vec::new(),
            insertions: 0,
            deletions: 0,
        });
    }

    let old_tree = repo.find_tree(old_tree_id).map_err(Error::repo)?;
    let new_tree = repo.find_tree(new_tree_id).map_err(Error::repo)?;
    let mut cache = repo
        .diff_resource_cache_for_tree_diff()
        .map_err(Error::repo)?;

    let mut files: Vec<FileStat> = Vec::new();
    let mut error: Option<Error> = None;

    old_tree
        .changes()
        .map_err(Error::repo)?
        .for_each_to_obtain_tree(&new_tree, |change| {
            let kind = classify(&change);
            let path = change.location().to_string();
            let from = match &change {
                Change::Rewrite {
                    source_location, ..
                } => Some(source_location.to_string()),
                _ => None,
            };

            if !change.entry_mode().is_no_tree() {
                // Directory entry — skip entirely, never push a FileStat.
                return Ok::<Action, std::convert::Infallible>(Action::Continue);
            }

            // The path filter runs before the blob work, not after: a
            // narrowed `ff diff src/` must not pay to diff the rest of the
            // tree it is not going to print.
            if !opts.paths.is_empty() && !crate::restore::path_selected(&path, &opts.paths) {
                return Ok::<Action, std::convert::Infallible>(Action::Continue);
            }

            let mut hunks = None;
            let (insertions, deletions, binary) = match change.diff(&mut cache) {
                Ok(mut platform) => {
                    let counted = match platform.line_counts() {
                        Ok(Some(counts)) => (counts.insertions, counts.removals, false),
                        Ok(None) => (0, 0, true),
                        Err(e) => {
                            error = Some(Error::repo(e));
                            (0, 0, false)
                        }
                    };
                    if opts.hunks && error.is_none() {
                        match crate::patch::hunks_of(&mut platform) {
                            // Binary: asked for, and there is no text to
                            // show. The empty vec says so; `None` would say
                            // nobody asked.
                            Ok(found) => hunks = Some(found.unwrap_or_default()),
                            Err(e) => error = Some(e),
                        }
                    }
                    counted
                }
                Err(e) => {
                    error = Some(Error::repo(e));
                    (0, 0, false)
                }
            };

            cache.clear_resource_cache();

            let (old_mode, new_mode, old_id, new_id) = if opts.hunks {
                sides(&change)
            } else {
                (None, None, None, None)
            };

            files.push(FileStat {
                path,
                from,
                kind,
                insertions,
                deletions,
                binary,
                hunks,
                old_mode,
                new_mode,
                old_id,
                new_id,
            });

            Ok::<Action, std::convert::Infallible>(Action::Continue)
        })
        .map_err(Error::repo)?;

    if let Some(e) = error {
        return Err(e);
    }

    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    let insertions = files.iter().map(|f| f.insertions).sum();
    let deletions = files.iter().map(|f| f.deletions).sum();

    Ok(ChangeStat {
        files,
        insertions,
        deletions,
    })
}
