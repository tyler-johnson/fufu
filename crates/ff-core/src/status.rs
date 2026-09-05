use gix::status::index_worktree;

use crate::error::{Error, Result};
use crate::model::{ChangeKind, Status, StatusEntry};

/// Compute the full status of the repository. Read-only: the worktree index is
/// never written back (`Outcome::write_changes` is deliberately never called),
/// so `.git/index` stays byte-identical.
pub fn status(repo: &gix::Repository) -> Result<Status> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: status requires a working copy",
            vec![],
        ));
    }
    let head = crate::head::head_state(repo)?;
    let operation = crate::head::operation(repo);
    let upstream = crate::upstream::upstream(repo)?;

    // git's own trick: a valid cache-tree root equal to HEAD's tree proves the
    // staged diff is empty, so the whole head-tree↔index comparison (which gix
    // otherwise materializes as an index) can be skipped.
    let head_tree_id = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let index = repo.index_or_empty().map_err(Error::repo)?;
    let staged_known_clean = index
        .tree()
        .is_some_and(|t| t.num_entries.is_some() && t.id == head_tree_id);
    drop(index);

    let platform = repo
        .status(gix::progress::Discard)
        .map_err(Error::repo)?
        // `git status` does not detect renames between index and worktree.
        .index_worktree_rewrites(None);
    let iter: Box<dyn Iterator<Item = Result<gix::status::Item>>> = if staged_known_clean {
        Box::new(
            platform
                .into_index_worktree_iter(Vec::new())
                .map_err(Error::repo)?
                .map(|res| {
                    res.map(gix::status::Item::IndexWorktree)
                        .map_err(Error::repo)
                }),
        )
    } else {
        Box::new(
            platform
                .into_iter(None::<gix::bstr::BString>)
                .map_err(Error::repo)?
                .map(|res| res.map_err(Error::repo)),
        )
    };

    let mut staged: Vec<StatusEntry> = Vec::new();
    let mut unstaged: Vec<StatusEntry> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();

    for item in iter {
        let item = item?;
        match item {
            gix::status::Item::TreeIndex(change) => {
                if let Some(entry) = tree_index_entry(change) {
                    staged.push(entry);
                }
            }
            gix::status::Item::IndexWorktree(item) => {
                use index_worktree::iter::Summary;
                let Some(summary) = item.summary() else {
                    continue;
                };
                match summary {
                    Summary::Conflict => conflicts.push(item_path(&item)),
                    Summary::Added => {
                        if let index_worktree::Item::DirectoryContents { entry, .. } = &item {
                            let mut path = entry.rela_path.to_string();
                            if matches!(
                                entry.disk_kind,
                                Some(
                                    gix::dir::entry::Kind::Directory
                                        | gix::dir::entry::Kind::Repository
                                )
                            ) {
                                path.push('/');
                            }
                            untracked.push(path);
                        }
                    }
                    Summary::Removed => unstaged.push(plain(&item, ChangeKind::Deleted)),
                    Summary::Modified => unstaged.push(plain(&item, ChangeKind::Modified)),
                    Summary::TypeChange => unstaged.push(plain(&item, ChangeKind::TypeChange)),
                    Summary::IntentToAdd => unstaged.push(plain(&item, ChangeKind::IntentToAdd)),
                    Summary::Renamed | Summary::Copied => {
                        if let index_worktree::Item::Rewrite {
                            source,
                            dirwalk_entry,
                            copy,
                            ..
                        } = &item
                        {
                            unstaged.push(StatusEntry {
                                path: dirwalk_entry.rela_path.to_string(),
                                from: Some(source.rela_path().to_string()),
                                kind: if *copy {
                                    ChangeKind::Copied
                                } else {
                                    ChangeKind::Renamed
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    // The head-tree vs index diff skips conflicted index entries entirely, which
    // makes conflicted paths surface as phantom staged deletions. git reports
    // such paths only as unmerged — mirror that.
    conflicts.sort();
    conflicts.dedup();
    staged.retain(|e| conflicts.binary_search(&e.path).is_err());
    unstaged.retain(|e| conflicts.binary_search(&e.path).is_err());

    // Item order is undefined under the parallel iterator: impose path order.
    staged.sort_by(|a, b| a.path.cmp(&b.path));
    unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    untracked.sort();

    Ok(Status {
        head,
        operation,
        upstream,
        staged,
        unstaged,
        untracked,
        conflicts,
    })
}

fn tree_index_entry(change: gix::diff::index::Change) -> Option<StatusEntry> {
    use gix::diff::index::Change;
    Some(match change {
        Change::Addition { location, .. } => StatusEntry {
            path: location.to_string(),
            from: None,
            kind: ChangeKind::Added,
        },
        Change::Deletion { location, .. } => StatusEntry {
            path: location.to_string(),
            from: None,
            kind: ChangeKind::Deleted,
        },
        Change::Modification {
            location,
            previous_entry_mode,
            entry_mode,
            ..
        } => StatusEntry {
            path: location.to_string(),
            from: None,
            kind: if mode_class(previous_entry_mode) != mode_class(entry_mode) {
                ChangeKind::TypeChange
            } else {
                ChangeKind::Modified
            },
        },
        Change::Rewrite {
            source_location,
            location,
            copy,
            ..
        } => StatusEntry {
            path: location.to_string(),
            from: Some(source_location.to_string()),
            kind: if copy {
                ChangeKind::Copied
            } else {
                ChangeKind::Renamed
            },
        },
    })
}

/// Collapse an index entry mode to its `git status` type class: the executable
/// bit is not a type change, but file↔symlink↔submodule transitions are.
fn mode_class(mode: gix::index::entry::Mode) -> u8 {
    use gix::index::entry::Mode;
    if mode.contains(Mode::SYMLINK) {
        1
    } else if mode.contains(Mode::COMMIT) {
        2
    } else if mode.contains(Mode::DIR) {
        3
    } else {
        0
    }
}

fn item_path(item: &index_worktree::Item) -> String {
    match item {
        index_worktree::Item::Modification { rela_path, .. } => rela_path.to_string(),
        index_worktree::Item::DirectoryContents { entry, .. } => entry.rela_path.to_string(),
        index_worktree::Item::Rewrite { dirwalk_entry, .. } => dirwalk_entry.rela_path.to_string(),
    }
}

fn plain(item: &index_worktree::Item, kind: ChangeKind) -> StatusEntry {
    StatusEntry {
        path: item_path(item),
        from: None,
        kind,
    }
}
