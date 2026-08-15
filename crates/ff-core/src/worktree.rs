//! Worktree materialization: apply the file-level difference between two
//! trees to the working directory. The single writer restore and switch
//! share — deletions prune emptied directories, blobs go through the filter
//! pipeline, writes are atomic (temp file + rename), gitlinks are reported
//! but never touched.

use std::io::Read;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use crate::error::{Error, Result};

/// What a transition did to the worktree.
#[derive(Debug, Default)]
pub(crate) struct Transition {
    /// Files written (created or overwritten) from the `to` tree.
    pub written: Vec<String>,
    /// Files deleted because the `to` tree does not contain them.
    pub deleted: Vec<String>,
    /// Gitlinks (embedded repositories) present in the diff but not touched.
    pub skipped_gitlinks: Vec<String>,
}

/// Transition the worktree from the state recorded in `from` to the state in
/// `to`, touching only paths accepted by `select`. The worktree is assumed to
/// currently match `from`; paths outside the diff are never visited.
pub(crate) fn apply_tree_transition(
    repo: &gix::Repository,
    from: gix::ObjectId,
    to: gix::ObjectId,
    select: &dyn Fn(&str) -> bool,
) -> Result<Transition> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| Error::coded("repo/bare", "bare repository: no worktree to write", vec![]))?
        .to_owned();
    let changes = tree_changes(repo, from, to)?;
    let mut out = Transition::default();
    let (mut pipeline, _index) = repo.filter_pipeline(None).map_err(Error::repo)?;

    for change in changes {
        if !select(&change.path) {
            continue;
        }
        match change.action {
            Action::DeleteFromWorktree => {
                let abs = workdir.join(&change.path);
                match std::fs::remove_file(&abs) {
                    Ok(()) => {
                        prune_empty_parents(&workdir, &abs);
                        out.deleted.push(change.path);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        out.deleted.push(change.path);
                    }
                    Err(err) => {
                        return Err(Error::msg(format!(
                            "could not delete {}: {err}",
                            change.path
                        )));
                    }
                }
            }
            Action::Materialize { oid, kind } => {
                use gix::objs::tree::EntryKind;
                match kind {
                    EntryKind::Commit => {
                        out.skipped_gitlinks.push(change.path);
                    }
                    EntryKind::Tree => {}
                    EntryKind::Link => {
                        let blob = repo.find_object(oid).map_err(Error::repo)?.detach();
                        let abs = workdir.join(&change.path);
                        if let Some(parent) = abs.parent() {
                            std::fs::create_dir_all(parent).map_err(Error::repo)?;
                        }
                        let _ = std::fs::remove_file(&abs);
                        #[cfg(unix)]
                        {
                            let target: PathBuf =
                                gix::path::from_bstr(gix::bstr::BStr::new(&blob.data)).into_owned();
                            std::os::unix::fs::symlink(&target, &abs).map_err(Error::repo)?;
                        }
                        // Windows: mirror git's core.symlinks=false default —
                        // a link entry materializes as a plain file holding
                        // the target path.
                        #[cfg(not(unix))]
                        std::fs::write(&abs, &blob.data).map_err(Error::repo)?;
                        out.written.push(change.path);
                    }
                    EntryKind::Blob | EntryKind::BlobExecutable => {
                        let blob = repo.find_object(oid).map_err(Error::repo)?.detach();
                        let converted = pipeline
                            .convert_to_worktree(
                                &blob.data,
                                change.path.as_str().into(),
                                gix::filter::plumbing::driver::apply::Delay::Forbid,
                            )
                            .map_err(Error::repo)?;
                        let bytes: Vec<u8> = match converted.as_bytes() {
                            Some(bytes) => bytes.to_vec(),
                            None => {
                                let mut buf = Vec::new();
                                let mut reader = converted;
                                reader.read_to_end(&mut buf).map_err(Error::repo)?;
                                buf
                            }
                        };
                        write_atomic(
                            &workdir,
                            &change.path,
                            &bytes,
                            matches!(kind, EntryKind::BlobExecutable),
                        )?;
                        out.written.push(change.path);
                    }
                }
            }
        }
    }

    out.written.sort();
    out.deleted.sort();
    out.skipped_gitlinks.sort();
    Ok(out)
}

enum Action {
    DeleteFromWorktree,
    Materialize {
        oid: gix::ObjectId,
        kind: gix::objs::tree::EntryKind,
    },
}

struct PathChange {
    path: String,
    action: Action,
}

/// File-level changes that carry the worktree from `from` to `to`: paths only
/// in `from` are deleted, everything else materializes `to`'s version.
fn tree_changes(
    repo: &gix::Repository,
    from: gix::ObjectId,
    to: gix::ObjectId,
) -> Result<Vec<PathChange>> {
    if from == to {
        return Ok(Vec::new());
    }
    let lhs = repo.find_object(from).map_err(Error::repo)?.detach();
    let rhs = repo.find_object(to).map_err(Error::repo)?.detach();
    let mut recorder = gix::diff::tree::Recorder::default();
    gix::diff::tree(
        gix::objs::TreeRefIter::from_bytes(&lhs.data),
        gix::objs::TreeRefIter::from_bytes(&rhs.data),
        gix::diff::tree::State::default(),
        &repo.objects,
        &mut recorder,
    )
    .map_err(Error::repo)?;

    use gix::diff::tree::recorder::Change as Rec;
    let mut out = Vec::new();
    for record in recorder.records {
        match record {
            // Only in `to`: materialize it.
            Rec::Addition {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if !entry_mode.is_tree() {
                    out.push(PathChange {
                        path: path.to_string(),
                        action: Action::Materialize {
                            oid,
                            kind: entry_mode.kind(),
                        },
                    });
                }
            }
            // Only in `from`: delete it.
            Rec::Deletion {
                entry_mode, path, ..
            } => {
                if !entry_mode.is_tree() {
                    out.push(PathChange {
                        path: path.to_string(),
                        action: Action::DeleteFromWorktree,
                    });
                }
            }
            Rec::Modification {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if !entry_mode.is_tree() {
                    out.push(PathChange {
                        path: path.to_string(),
                        action: Action::Materialize {
                            oid,
                            kind: entry_mode.kind(),
                        },
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Atomic materialization: temp file in the same directory, then rename.
pub(crate) fn write_atomic(
    workdir: &Path,
    rela: &str,
    bytes: &[u8],
    executable: bool,
) -> Result<()> {
    let abs = workdir.join(rela);
    let parent = abs.parent().unwrap_or(workdir).to_owned();
    std::fs::create_dir_all(&parent).map_err(Error::repo)?;
    let tmp = parent.join(format!(
        ".ff-restore-{}-{}",
        std::process::id(),
        abs.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes).map_err(Error::repo)?;
    // The exec bit only exists on unix; on Windows git ignores worktree mode.
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(Error::repo)?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    // A symlink or directory may occupy the destination: clear it.
    if let Ok(md) = std::fs::symlink_metadata(&abs)
        && md.is_dir()
    {
        std::fs::remove_dir_all(&abs).map_err(Error::repo)?;
    }
    if let Err(err) = std::fs::rename(&tmp, &abs) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::msg(format!("could not write {rela}: {err}")));
    }
    Ok(())
}

/// Remove now-empty parent directories, bottom-up, stopping at the worktree
/// root (and never touching `.git`).
pub(crate) fn prune_empty_parents(workdir: &Path, deleted: &Path) {
    let mut dir = deleted.parent();
    while let Some(d) = dir {
        if d == workdir {
            break;
        }
        match std::fs::read_dir(d) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}
