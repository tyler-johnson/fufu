//! Snapshot tree assembly. gix 0.73 has no index→tree writer, so the capture
//! tree is built as: HEAD tree + staged deltas + worktree deltas + untracked,
//! which is exactly what `read-tree HEAD && add -A && write-tree` produces —
//! the differential harness holds that equality permanently.

use std::collections::BTreeSet;

use gix::status::index_worktree;

use crate::error::{Error, Result};

/// Everything the capture status scan found, bucketed for assembly.
/// Empty scan ⇒ the capture tree IS the head tree (tier-1 shortcut).
#[derive(Debug, Default)]
pub(crate) struct Scan {
    /// Index-side content to lay over the head tree: (path, kind, id).
    /// Ids come from the index and exist in the odb by construction.
    pub staged_upserts: Vec<(String, gix::objs::tree::EntryKind, gix::ObjectId)>,
    /// Paths staged for deletion (conflicted paths already subtracted —
    /// gix's tree↔index diff skips unmerged entries, which would otherwise
    /// surface conflicts as phantom staged deletions).
    pub staged_deletes: BTreeSet<String>,
    /// Tracked paths whose worktree content must be rehashed through the
    /// filter pipeline: modified/typechange/intent-to-add ∪ conflicted.
    pub rehash: BTreeSet<String>,
    /// Untracked paths — separate from `rehash` so stash can commit them
    /// apart; capture treats both buckets identically.
    pub untracked: BTreeSet<String>,
    /// Paths deleted in the worktree (minus conflicted).
    pub wt_deletes: BTreeSet<String>,
}

impl Scan {
    pub fn is_empty(&self) -> bool {
        self.staged_upserts.is_empty()
            && self.staged_deletes.is_empty()
            && self.rehash.is_empty()
            && self.untracked.is_empty()
            && self.wt_deletes.is_empty()
    }

    /// Every path this scan touches, in no particular order — the union of
    /// all five buckets. What the worktree has that HEAD does not, whichever
    /// way the difference runs.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.staged_upserts
            .iter()
            .map(|(path, ..)| path.as_str())
            .chain(self.staged_deletes.iter().map(String::as_str))
            .chain(self.rehash.iter().map(String::as_str))
            .chain(self.untracked.iter().map(String::as_str))
            .chain(self.wt_deletes.iter().map(String::as_str))
    }

    /// The same scan, restricted to the entries
    /// [`crate::restore::path_selected`] picks out of `paths`. Empty `paths`
    /// return it untouched.
    ///
    /// This works without touching [`assemble`]: that reads whatever `Scan` it
    /// is handed and commits one bucket apart from another anyway, so narrowing
    /// is a property of the scan, not a new assembly mode.
    pub(crate) fn narrowed(self, paths: &[String]) -> Scan {
        if paths.is_empty() {
            return self;
        }
        let picked = |path: &str| crate::restore::path_selected(path, paths);
        Scan {
            staged_upserts: self
                .staged_upserts
                .into_iter()
                .filter(|(path, ..)| picked(path))
                .collect(),
            staged_deletes: self
                .staged_deletes
                .into_iter()
                .filter(|path| picked(path))
                .collect(),
            rehash: self
                .rehash
                .into_iter()
                .filter(|path| picked(path))
                .collect(),
            untracked: self
                .untracked
                .into_iter()
                .filter(|path| picked(path))
                .collect(),
            wt_deletes: self
                .wt_deletes
                .into_iter()
                .filter(|path| picked(path))
                .collect(),
        }
    }
}

fn entry_kind(mode: gix::index::entry::Mode) -> Result<gix::objs::tree::EntryKind> {
    mode.to_tree_entry_mode()
        .map(|m| m.kind())
        .ok_or_else(|| Error::msg(format!("index entry mode {mode:?} has no tree equivalent")))
}

/// Run capture's own status scan (config differs from `ff status`: no rename
/// tracking anywhere, untracked emitted per file). Read-only.
pub(crate) fn scan(repo: &gix::Repository) -> Result<Scan> {
    let head_tree_id = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let index = repo.index_or_empty().map_err(Error::repo)?;
    if index.is_sparse() {
        return Err(Error::msg(
            "sparse checkout: fufu cannot capture sparse worktrees yet",
        ));
    }
    // Conflicted paths carry only stage>0 entries; `add -A` stage-0s their
    // worktree side, so they join the rehash list unconditionally.
    let mut conflicted: BTreeSet<String> = BTreeSet::new();
    for entry in index.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            conflicted.insert(entry.path(&index).to_string());
        }
    }
    // Same cache-tree shortcut as status.rs: a valid root equal to HEAD's
    // tree proves the staged diff empty.
    let staged_known_clean = index
        .tree()
        .is_some_and(|t| t.num_entries.is_some() && t.id == head_tree_id);
    drop(index);

    let platform = repo
        .status(gix::progress::Discard)
        .map_err(Error::repo)?
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .untracked_files(gix::status::UntrackedFiles::Files);
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

    let mut scan = Scan::default();
    for item in iter {
        match item? {
            gix::status::Item::TreeIndex(change) => {
                use gix::diff::index::Change;
                match change {
                    Change::Addition {
                        location,
                        entry_mode,
                        id,
                        ..
                    }
                    | Change::Modification {
                        location,
                        entry_mode,
                        id,
                        ..
                    } => {
                        let path = location.to_string();
                        if !conflicted.contains(&path) {
                            scan.staged_upserts.push((
                                path,
                                entry_kind(entry_mode)?,
                                id.into_owned(),
                            ));
                        }
                    }
                    Change::Deletion { location, .. } => {
                        let path = location.to_string();
                        if !conflicted.contains(&path) {
                            scan.staged_deletes.insert(path);
                        }
                    }
                    // Rename tracking is disabled; a Rewrite cannot appear.
                    Change::Rewrite {
                        source_location,
                        location,
                        entry_mode,
                        id,
                        ..
                    } => {
                        scan.staged_deletes.insert(source_location.to_string());
                        scan.staged_upserts.push((
                            location.to_string(),
                            entry_kind(entry_mode)?,
                            id.into_owned(),
                        ));
                    }
                }
            }
            gix::status::Item::IndexWorktree(item) => {
                use index_worktree::iter::Summary;
                let Some(summary) = item.summary() else {
                    continue;
                };
                match summary {
                    // Conflicts were already collected from the index; the
                    // worktree side of a conflicted path is handled below.
                    Summary::Conflict => {}
                    Summary::Added => {
                        if let index_worktree::Item::DirectoryContents { entry, .. } = &item {
                            // Plain directories can't be tracked; embedded
                            // repositories become gitlinks via the rehash.
                            if !matches!(entry.disk_kind, Some(gix::dir::entry::Kind::Directory)) {
                                scan.untracked.insert(entry.rela_path.to_string());
                            }
                        }
                    }
                    Summary::Modified | Summary::TypeChange | Summary::IntentToAdd => {
                        scan.rehash.insert(item_path(&item));
                    }
                    Summary::Removed => {
                        let path = item_path(&item);
                        if !conflicted.contains(&path) {
                            scan.wt_deletes.insert(path);
                        }
                    }
                    // index_worktree_rewrites(None) disables these.
                    Summary::Renamed | Summary::Copied => {
                        return Err(Error::msg("unexpected rename in capture scan"));
                    }
                }
            }
        }
    }

    // A conflicted path with no worktree change still needs its worktree side
    // captured (or, if gone from disk, removed) — `add -A` semantics.
    scan.rehash.extend(conflicted);
    Ok(scan)
}

fn item_path(item: &index_worktree::Item) -> String {
    match item {
        index_worktree::Item::Modification { rela_path, .. } => rela_path.to_string(),
        index_worktree::Item::DirectoryContents { entry, .. } => entry.rela_path.to_string(),
        index_worktree::Item::Rewrite { dirwalk_entry, .. } => dirwalk_entry.rela_path.to_string(),
    }
}

/// Apply a scan to the base (HEAD) tree and write the snapshot tree.
/// Returns the tree id and the oversize-skipped paths. Only object writes
/// happen here — no refs, no index.
pub(crate) fn assemble(
    repo: &gix::Repository,
    base_tree: gix::ObjectId,
    scan: &Scan,
    max_file_size: u64,
) -> Result<(gix::ObjectId, Vec<String>)> {
    let mut editor = repo.edit_tree(base_tree).map_err(Error::repo)?;
    for (path, kind, id) in &scan.staged_upserts {
        editor
            .upsert(path.as_str(), *kind, *id)
            .map_err(Error::repo)?;
    }
    for path in &scan.staged_deletes {
        editor.remove(path.as_str()).map_err(Error::repo)?;
    }

    let mut skipped = Vec::new();
    // Capture makes no tracked/untracked distinction: both buckets rehash.
    // The merged iteration stays sorted (the filter pipeline stats attribute
    // files per directory change, so sorted order keeps that cheap).
    let mut rehash_all: Vec<&String> = scan.rehash.iter().chain(scan.untracked.iter()).collect();
    rehash_all.sort();
    if !rehash_all.is_empty() {
        let workdir = repo
            .workdir()
            .ok_or_else(|| Error::coded("repo/bare", "bare repository: cannot capture", vec![]))?
            .to_owned();
        let (mut pipeline, index) = repo.filter_pipeline(None).map_err(Error::repo)?;
        for path in rehash_all {
            let abs = workdir.join(path);
            if let Ok(md) = std::fs::symlink_metadata(&abs) {
                // Only regular files are size-capped; user-staged big blobs
                // were captured above as-is — excluding them would be forgery.
                if md.is_file() && md.len() > max_file_size {
                    skipped.push(path.clone());
                    continue;
                }
            }
            match pipeline
                .worktree_file_to_object(path.as_str().into(), &index)
                .map_err(Error::repo)?
            {
                Some((id, kind, _md)) => {
                    editor
                        .upsert(path.as_str(), kind, id)
                        .map_err(Error::repo)?;
                }
                // Vanished between scan and hash — or a conflicted path
                // deleted from disk: gone from the snapshot too.
                None => {
                    editor.remove(path.as_str()).map_err(Error::repo)?;
                }
            }
        }
    }

    for path in &scan.wt_deletes {
        editor.remove(path.as_str()).map_err(Error::repo)?;
    }
    let tree = editor.write().map_err(Error::repo)?.detach();
    Ok((tree, skipped))
}
