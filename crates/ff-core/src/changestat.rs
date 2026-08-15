//! Per-file insertions and deletions for the open change.
//!
//! The capture chain tip's tree is the worktree as a git tree object, so the
//! diffstat is a plain tree-to-tree diff: HEAD's tree against the tip's tree.

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

/// Compute the diffstat of the open change: HEAD's tree against the capture
/// chain tip's tree. Returns an empty result when there is no chain tip, the
/// trees are identical, or the repository is bare.
pub fn change_stat(repo: &gix::Repository) -> Result<ChangeStat> {
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

    let tip_id = crate::snapshot::chain::tip(
        repo,
        &format!("{}{branch}", crate::snapshot::chain::SNAP_PREFIX),
    )?;

    let Some(tip_id) = tip_id else {
        return Ok(ChangeStat {
            files: Vec::new(),
            insertions: 0,
            deletions: 0,
        });
    };

    let tip_tree_id = repo
        .find_commit(tip_id)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();

    // Identical trees: nothing to report.
    if tip_tree_id == head_tree_id {
        return Ok(ChangeStat {
            files: Vec::new(),
            insertions: 0,
            deletions: 0,
        });
    }

    let old_tree = repo.find_tree(head_tree_id).map_err(Error::repo)?;
    let new_tree = repo.find_tree(tip_tree_id).map_err(Error::repo)?;
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

            let (insertions, deletions, binary) = match change.diff(&mut cache) {
                Ok(mut platform) => match platform.line_counts() {
                    Ok(Some(counts)) => (counts.insertions, counts.removals, false),
                    Ok(None) => (0, 0, true),
                    Err(e) => {
                        error = Some(Error::repo(e));
                        (0, 0, false)
                    }
                },
                Err(e) => {
                    error = Some(Error::repo(e));
                    (0, 0, false)
                }
            };

            cache.clear_resource_cache();

            files.push(FileStat {
                path,
                from,
                kind,
                insertions,
                deletions,
                binary,
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
