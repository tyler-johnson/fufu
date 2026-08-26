//! Index writing: rebuild `.git/index` to match a tree, natively. gix's
//! `index_from_tree` is a real `read-tree` — every entry arrives with zeroed
//! stat data — so stats are carried over from the previous index wherever
//! (path, id, mode) survived, or the first status after a switch would rehash
//! the whole worktree.
//!
//! gix cannot attach a TREE (cache tree) extension to a `State`, but the
//! extension's struct and serializer are public — so the cache tree is
//! synthesized here from the tree object itself and spliced between the
//! serialized entries and the trailer. A fresh-from-tree index is the one
//! case where the whole cache tree is valid by construction, and it keeps
//! Phase 1's staged-known-clean shortcut alive across fufu's own writes.

use std::io::Write as _;

use crate::error::{Error, Result};

/// Rewrite the worktree index to exactly `tree`, preserving stat data from
/// the current on-disk index where entries are unchanged. Goes through
/// `index.lock` (fail-fast, atomic rename), honoring the repository's real
/// `index.skipHash` — not the read-path override this library opens with.
pub fn write_index_for_tree(repo: &gix::Repository, tree: gix::ObjectId) -> Result<()> {
    write_index_for_tree_except(repo, tree, &[])
}

/// [`write_index_for_tree`], for the case where the worktree is *known* not
/// to match the tree: `worktree_differs` names the paths where it does not,
/// and their entries keep zeroed stats so the next status must hash them.
///
/// Carrying stat data over is only sound when the file on disk really is the
/// entry being written, which every caller but one can take for granted —
/// they write an index for the tree they just put in the worktree. A partial
/// `ff commit <paths>` is the exception: it writes the index to the commit
/// while deliberately leaving the unselected edits on disk. For those paths
/// the previous index and the new entry agree (both hold HEAD's blob, which
/// the slice did not touch), so the id-and-mode test matches and the *stale*
/// stat rides along. The next status then trusts that stat and never opens
/// the file, and the remainder silently stops being the open change.
///
/// Filesystem timestamp resolution decides whether that is visible: on ext4
/// the carried mtime differs from the edited file's and gix rehashes anyway,
/// which is why this was invisible on Linux and macOS and lost the open
/// change on Windows every time.
pub fn write_index_for_tree_except(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    worktree_differs: &[String],
) -> Result<()> {
    let mut file = repo.index_from_tree(&tree).map_err(Error::repo)?;

    if let Some(prev) = repo.try_index().map_err(Error::repo)? {
        carry_over_stats(&mut file, &prev, worktree_differs);
    }

    let skip_hash = repo
        .config_snapshot()
        .plumbing()
        .boolean_filter("index.skipHash", &mut |md: &gix::config::file::Metadata| {
            md.source != gix::config::Source::Api
        })
        .and_then(|res| res.ok())
        .unwrap_or_default();

    // Serialize entries without extensions, splice in the synthesized cache
    // tree, then the trailer (zeroed under skipHash, real hash otherwise).
    let mut bytes = Vec::with_capacity(64 * 1024);
    let state: &gix::index::State = &file;
    state
        .write_to(
            &mut bytes,
            gix::index::write::Options {
                extensions: gix::index::write::Extensions::None,
                skip_hash,
            },
        )
        .map_err(Error::repo)?;
    cache_tree_for(repo, tree)?
        .write_to(&mut bytes)
        .map_err(Error::repo)?;
    if skip_hash {
        bytes.extend_from_slice(repo.object_hash().null().as_slice());
    } else {
        let mut hasher = gix::hash::hasher(repo.object_hash());
        hasher.update(&bytes);
        let digest = hasher.try_finalize().map_err(Error::repo)?;
        bytes.extend_from_slice(digest.as_slice());
    }

    write_locked(file.path(), &bytes)
}

/// Git's own lock protocol: `index.lock` created exclusively, written,
/// synced, renamed into place. A held lock fails immediately.
fn write_locked(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let lock = path.with_extension("lock");
    let mut lock_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|err| Error::msg(format!("index is locked: {err}")))?;
    let write = lock_file
        .write_all(bytes)
        .and_then(|()| lock_file.sync_all())
        .and_then(|()| {
            drop(lock_file);
            std::fs::rename(&lock, path)
        });
    if let Err(err) = write {
        let _ = std::fs::remove_file(&lock);
        return Err(Error::msg(format!("could not write index: {err}")));
    }
    Ok(())
}

/// The index as it was found, restored on drop unless disarmed. The close
/// writes the index to the tree it is about to commit before running hooks,
/// so hook-runners keyed on `git diff --cached` see the change staged; if
/// the close then doesn't land, git rolls its own index back and so does
/// this.
///
/// Byte-exact rather than a rewrite to `head_tree`: a user may have their
/// own `git add` state, which fufu tolerates, and a close that refuses must
/// not destroy it.
pub struct IndexBackup {
    path: std::path::PathBuf,
    /// The file's bytes, or `None` when there was no index file at all
    /// (an unborn or freshly-initialized repository).
    bytes: Option<Vec<u8>>,
    armed: bool,
}

impl IndexBackup {
    /// Read the current index aside. In a linked worktree `index_path()`
    /// resolves per-worktree, which is the path this restores to.
    pub fn take(repo: &gix::Repository) -> Result<Self> {
        let path = repo.index_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(Error::msg(format!("could not read index: {err}"))),
        };
        Ok(Self {
            path,
            bytes,
            armed: true,
        })
    }

    /// Keep the index as it now stands — the close landed.
    pub fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for IndexBackup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Drop cannot propagate. A held `index.lock` means no restore, and
        // the next fufu operation rewrites the index anyway.
        match &self.bytes {
            Some(bytes) => {
                let _ = write_locked(&self.path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

/// Copy stat data for every entry whose (path, id, mode) match the previous
/// index at stage 0, except the paths the caller says the worktree differs
/// on. Everything else keeps zeroed stats and will be rehashed by the next
/// status — correct, just slower, and the only safe answer when the file on
/// disk is not the entry being written.
fn carry_over_stats(
    next: &mut gix::index::File,
    prev: &gix::index::File,
    worktree_differs: &[String],
) {
    for (entry, path) in next.entries_mut_with_paths() {
        // The id-and-mode test below cannot see this: for a path the slice
        // left behind, the previous index and the new entry hold the same
        // blob and only the worktree has moved on.
        if !worktree_differs.is_empty()
            && worktree_differs
                .iter()
                .any(|p| p.as_bytes() == AsRef::<[u8]>::as_ref(path))
        {
            continue;
        }
        if let Some(old) = prev.entry_by_path(path)
            && old.stage() == gix::index::entry::Stage::Unconflicted
            && old.id == entry.id
            && old.mode == entry.mode
        {
            entry.stat = old.stat;
        }
    }
}

/// The tree the current index describes — what `git write-tree` would
/// produce. The valid cache-tree root answers instantly; otherwise the tree
/// is built from the stage-0 entries (conflicted paths carry no stage-0
/// entry and are therefore absent, exactly as `write-tree` would refuse —
/// callers in a conflicted repo get the unconflicted projection instead).
pub fn tree_from_index(repo: &gix::Repository) -> Result<gix::ObjectId> {
    let index = repo.index_or_empty().map_err(Error::repo)?;
    if index.is_sparse() {
        return Err(Error::msg(
            "sparse checkout: fufu cannot operate on sparse worktrees yet",
        ));
    }
    if let Some(tree) = index.tree()
        && tree.num_entries.is_some()
        && tree.name.is_empty()
    {
        return Ok(tree.id);
    }
    let empty = gix::ObjectId::empty_tree(repo.object_hash());
    let mut editor = repo.edit_tree(empty).map_err(Error::repo)?;
    for entry in index.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            continue;
        }
        let kind = entry
            .mode
            .to_tree_entry_mode()
            .map(|m| m.kind())
            .ok_or_else(|| {
                Error::msg(format!(
                    "index entry mode {:?} has no tree equivalent",
                    entry.mode
                ))
            })?;
        editor
            .upsert(entry.path(&index), kind, entry.id)
            .map_err(Error::repo)?;
    }
    Ok(editor.write().map_err(Error::repo)?.detach())
}

/// Build the full cache tree for a tree object: every node valid, counts
/// covering blobs, symlinks, and gitlinks recursively. Children are sorted
/// by raw name bytes — cache-tree order, which differs from tree-object
/// order (where directories sort as `name/`).
fn cache_tree_for(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> Result<gix::index::extension::Tree> {
    fn walk(
        repo: &gix::Repository,
        id: gix::ObjectId,
        name: &[u8],
    ) -> Result<gix::index::extension::Tree> {
        let obj = repo.find_object(id).map_err(Error::repo)?.detach();
        let iter = gix::objs::TreeRefIter::from_bytes(&obj.data);
        let mut children = Vec::new();
        let mut entries: u32 = 0;
        for entry in iter {
            let entry = entry.map_err(Error::repo)?;
            if entry.mode.is_tree() {
                let child = walk(repo, entry.oid.to_owned(), entry.filename)?;
                entries += child.num_entries.unwrap_or(0);
                children.push(child);
            } else {
                entries += 1;
            }
        }
        children.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(gix::index::extension::Tree {
            name: name.into(),
            id,
            num_entries: Some(entries),
            children,
        })
    }
    walk(repo, tree, b"")
}
