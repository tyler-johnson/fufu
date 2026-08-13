//! Worktree restore from the timeline. Load-bearing ordering: resolve the
//! target FIRST (so `@{1}` and "newest" mean the timeline the user just
//! looked at), then take the mandatory pre-restore snapshot, then write.
//! Writes touch only the worktree — never the index, HEAD, or branches.

use std::io::Read;
use std::path::{Path, PathBuf};

use gix::prelude::ObjectIdExt;

use crate::error::{Error, Result};
use crate::model::{RestoreReport, SnapEntry, SnapOutcome};
use crate::snapshot::chain;
use crate::snapshot::{Provenance, TakeOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreTarget {
    /// The newest snapshot on the chain (the default).
    Newest,
    /// A snapshot id prefix, resolved across the live and trash chains.
    Id(String),
    /// `@{n}` — the reflog entry n steps back.
    Back(usize),
    /// The chain as of a moment in time (`@{<date>}` semantics).
    AtTime(i64),
}

/// Parse the target grammar: nothing, a hex prefix, `@{n}`, a compact
/// duration (`90s`/`15m`/`2h`/`3d`/`1w`), or a git-style date string.
pub fn parse_target(raw: Option<&str>, now: i64) -> Result<RestoreTarget> {
    let Some(raw) = raw else {
        return Ok(RestoreTarget::Newest);
    };
    let raw = raw.trim();
    if let Some(n) = raw
        .strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .and_then(|s| s.parse::<usize>().ok())
    {
        return Ok(RestoreTarget::Back(n));
    }
    if let Some(secs) = parse_compact_age(raw) {
        return Ok(RestoreTarget::AtTime(now - secs));
    }
    if raw.len() >= 4 && raw.len() <= 40 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(RestoreTarget::Id(raw.to_ascii_lowercase()));
    }
    let time = gix::date::parse(raw, Some(std::time::SystemTime::now()))
        .map_err(|err| Error::msg(format!("unrecognized restore target {raw:?}: {err}")))?;
    Ok(RestoreTarget::AtTime(time.seconds))
}

/// `<n><unit>` with a mandatory unit — bare integers are ambiguous here.
fn parse_compact_age(raw: &str) -> Option<i64> {
    let unit = raw.chars().last()?;
    if !matches!(unit, 's' | 'm' | 'h' | 'd' | 'w') {
        return None;
    }
    let n: i64 = raw[..raw.len() - 1].parse().ok()?;
    Some(match unit {
        's' => n,
        'm' => n * 60,
        'h' => n * 3600,
        'd' => n * 86_400,
        'w' => n * 7 * 86_400,
        _ => unreachable!(),
    })
}

#[derive(Debug, Clone, Default)]
pub struct RestoreOptions {
    /// Raw target string (see [`parse_target`]); `None` = newest.
    pub target: Option<String>,
    /// Restore only these repo-relative paths (files, or directory prefixes).
    /// Empty + `all` = the whole worktree.
    pub paths: Vec<String>,
    pub all: bool,
    /// Clock injection for tests.
    pub now: Option<i64>,
}

/// Resolve, snapshot, write. `prov` names the mandatory pre-restore snapshot
/// (`pre: ff restore …`); if that snapshot is contended the restore aborts.
pub fn restore(
    repo: &gix::Repository,
    opts: &RestoreOptions,
    prov: &Provenance,
) -> Result<RestoreReport> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| Error::msg("bare repository: nothing to restore into"))?
        .to_owned();
    if !opts.all && opts.paths.is_empty() {
        return Err(Error::msg("nothing selected: pass paths or --all"));
    }
    let head = crate::head::head_state(repo)?;
    let chain_name = chain::chain_name(&head);

    let now = opts.now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    // 1. Resolve first: "newest" and @{n} refer to the timeline as the user
    //    saw it, before the pre-restore snapshot moves the tip.
    let target = parse_target(opts.target.as_deref(), now)?;
    let target_id = resolve(repo, &target, &chain_name)?;
    let target_entry = entry_of(repo, target_id)?;

    // 2. Mandatory pre-restore snapshot: the state being overwritten must be
    //    on the timeline before a single byte moves.
    let pre = crate::snapshot::take_with(
        repo,
        prov,
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )?;
    let (fresh_tree, pre_snapshot) = match &pre {
        SnapOutcome::Created { id, .. } => {
            let id = gix::ObjectId::from_hex(id.as_bytes()).map_err(Error::repo)?;
            (
                repo.find_commit(id)
                    .map_err(Error::repo)?
                    .tree_id()
                    .map_err(Error::repo)?
                    .detach(),
                Some(id.to_string()),
            )
        }
        SnapOutcome::NoOp { tip: Some(tip), .. } => {
            let id = gix::ObjectId::from_hex(tip.as_bytes()).map_err(Error::repo)?;
            (
                repo.find_commit(id)
                    .map_err(Error::repo)?
                    .tree_id()
                    .map_err(Error::repo)?
                    .detach(),
                None,
            )
        }
        SnapOutcome::NoOp { tip: None, .. } => (
            repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
            None,
        ),
        SnapOutcome::Contended { .. } => {
            return Err(Error::msg(
                "a concurrent ff snapshot is in progress; restore aborted (nothing was written)",
            ));
        }
    };

    // 3. Diff target ↔ fresh snapshot — never target ↔ worktree, where
    //    untracked files would read as deletions.
    let target_tree = repo
        .find_commit(target_id)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();
    let changes = tree_changes(repo, target_tree, fresh_tree)?;

    let mut report = RestoreReport {
        target: target_entry,
        restored: Vec::new(),
        deleted: Vec::new(),
        skipped_gitlinks: Vec::new(),
        pre_snapshot,
    };
    let (mut pipeline, _index) = repo.filter_pipeline(None).map_err(Error::repo)?;

    for change in changes {
        if !opts.all && !path_selected(&change.path, &opts.paths) {
            continue;
        }
        match change.action {
            Action::DeleteFromWorktree => {
                let abs = workdir.join(&change.path);
                match std::fs::remove_file(&abs) {
                    Ok(()) => {
                        prune_empty_parents(&workdir, &abs);
                        report.deleted.push(change.path);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        report.deleted.push(change.path);
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
                        report.skipped_gitlinks.push(change.path);
                    }
                    EntryKind::Tree => {}
                    EntryKind::Link => {
                        let blob = repo.find_object(oid).map_err(Error::repo)?.detach();
                        let target: PathBuf =
                            gix::path::from_bstr(gix::bstr::BStr::new(&blob.data)).into_owned();
                        let abs = workdir.join(&change.path);
                        if let Some(parent) = abs.parent() {
                            std::fs::create_dir_all(parent).map_err(Error::repo)?;
                        }
                        let _ = std::fs::remove_file(&abs);
                        std::os::unix::fs::symlink(&target, &abs).map_err(Error::repo)?;
                        report.restored.push(change.path);
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
                                let mut out = Vec::new();
                                let mut reader = converted;
                                reader.read_to_end(&mut out).map_err(Error::repo)?;
                                out
                            }
                        };
                        write_atomic(
                            &workdir,
                            &change.path,
                            &bytes,
                            matches!(kind, EntryKind::BlobExecutable),
                        )?;
                        report.restored.push(change.path);
                    }
                }
            }
        }
    }

    report.restored.sort();
    report.deleted.sort();
    report.skipped_gitlinks.sort();
    Ok(report)
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

/// File-level changes to apply to the worktree so it matches `target`,
/// given the current state `fresh`.
fn tree_changes(
    repo: &gix::Repository,
    target: gix::ObjectId,
    fresh: gix::ObjectId,
) -> Result<Vec<PathChange>> {
    if target == fresh {
        return Ok(Vec::new());
    }
    let lhs = repo.find_object(target).map_err(Error::repo)?.detach();
    let rhs = repo.find_object(fresh).map_err(Error::repo)?.detach();
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
            // Present now, absent in the target: delete.
            Rec::Addition {
                entry_mode, path, ..
            } => {
                if !entry_mode.is_tree() {
                    out.push(PathChange {
                        path: path.to_string(),
                        action: Action::DeleteFromWorktree,
                    });
                }
            }
            // In the target, absent now: materialize the target's version.
            Rec::Deletion {
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
            Rec::Modification {
                previous_entry_mode,
                previous_oid,
                path,
                ..
            } => {
                if !previous_entry_mode.is_tree() {
                    out.push(PathChange {
                        path: path.to_string(),
                        action: Action::Materialize {
                            oid: previous_oid,
                            kind: previous_entry_mode.kind(),
                        },
                    });
                }
            }
        }
    }
    Ok(out)
}

fn path_selected(path: &str, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| {
        let sel = sel.trim_end_matches('/');
        path == sel || path.starts_with(&format!("{sel}/"))
    })
}

/// Atomic materialization: temp file in the same directory, then rename.
fn write_atomic(workdir: &Path, rela: &str, bytes: &[u8], executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
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
    if executable {
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(Error::repo)?;
    }
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
fn prune_empty_parents(workdir: &Path, deleted: &Path) {
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

fn entry_of(repo: &gix::Repository, id: gix::ObjectId) -> Result<SnapEntry> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    if !chain::is_snapshot_commit(&commit) {
        return Err(Error::msg(format!(
            "{id} is not a fufu snapshot; restore only restores from the timeline"
        )));
    }
    let subject = commit.message().summary().to_string();
    let time = commit.committer.time().map_err(Error::repo)?.seconds;
    let parents: Vec<gix::ObjectId> = commit.parents().collect();
    drop(commit);
    drop(obj);
    let (prev, base) = match parents.as_slice() {
        [] => (None, None),
        [p1, rest @ ..] => {
            if chain::id_is_snapshot(repo, *p1)? {
                (Some(*p1), rest.first().copied())
            } else {
                (None, Some(*p1))
            }
        }
    };
    let short_id = id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string());
    Ok(SnapEntry {
        id: id.to_string(),
        short_id,
        subject,
        time,
        base: base.map(|b| b.to_string()),
        prev: prev.map(|p| p.to_string()),
    })
}

/// Resolve a target against the chain. Every exit is identity-guarded: the
/// resolved commit must bear the fufu identity or the restore refuses.
fn resolve(
    repo: &gix::Repository,
    target: &RestoreTarget,
    chain_name: &str,
) -> Result<gix::ObjectId> {
    let snap_ref = format!("{}{chain_name}", chain::SNAP_PREFIX);
    let id = match target {
        RestoreTarget::Newest => chain_tip(repo, &snap_ref)?.ok_or_else(|| {
            Error::msg(format!(
                "no snapshots on {chain_name} yet — nothing to restore"
            ))
        })?,
        RestoreTarget::Id(prefix) => {
            let mut candidates = Vec::new();
            for r in [snap_ref.clone(), chain::trash_ref(chain_name)] {
                if let Some(tip) = chain_tip(repo, &r)? {
                    collect_prefix_matches(repo, tip, prefix, &mut candidates)?;
                }
            }
            candidates.dedup();
            match candidates.as_slice() {
                [] => {
                    return Err(Error::msg(format!(
                        "no snapshot matching {prefix} on {chain_name} (or its trash)"
                    )));
                }
                [one] => *one,
                many => {
                    let list: Vec<String> = many.iter().map(|id| id.to_string()).collect();
                    return Err(Error::msg(format!(
                        "ambiguous snapshot prefix {prefix}: {}",
                        list.join(", ")
                    )));
                }
            }
        }
        RestoreTarget::Back(n) => reflog_entry(repo, &snap_ref, |lines| lines.nth(*n))?
            .ok_or_else(|| {
                Error::msg(format!("@{{{n}}}: not that many snapshots on {chain_name}"))
            })?,
        RestoreTarget::AtTime(t) => {
            reflog_entry(repo, &snap_ref, |lines| {
                // A manual find: `Iterator::find` needs `Self: Sized`.
                loop {
                    match lines.next() {
                        Some(line) if line.1 <= *t => break Some(line),
                        Some(_) => continue,
                        None => break None,
                    }
                }
            })?
            .ok_or_else(|| {
                Error::msg(format!(
                    "no snapshot on {chain_name} at or before that time"
                ))
            })?
        }
    };
    if !chain::id_is_snapshot(repo, id)? {
        return Err(Error::msg(format!(
            "{id} is not a fufu snapshot; refusing to restore from it"
        )));
    }
    Ok(id)
}

fn chain_tip(repo: &gix::Repository, ref_name: &str) -> Result<Option<gix::ObjectId>> {
    Ok(repo
        .try_find_reference(ref_name)
        .map_err(Error::repo)?
        .and_then(|r| r.target().try_id().map(|id| id.to_owned())))
}

/// First-parent walk collecting snapshot ids that match a hex prefix.
fn collect_prefix_matches(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    prefix: &str,
    out: &mut Vec<gix::ObjectId>,
) -> Result<()> {
    let mut cur = Some(tip);
    while let Some(id) = cur {
        if !chain::id_is_snapshot(repo, id)? {
            break;
        }
        if id.to_string().starts_with(prefix) && !out.contains(&id) {
            out.push(id);
        }
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
        cur = commit.parents().next();
    }
    Ok(())
}

/// Run a selector over reflog lines, newest first, as `(new_oid, time)`.
fn reflog_entry(
    repo: &gix::Repository,
    ref_name: &str,
    select: impl FnOnce(&mut dyn Iterator<Item = (gix::ObjectId, i64)>) -> Option<(gix::ObjectId, i64)>,
) -> Result<Option<gix::ObjectId>> {
    let Some(reference) = repo.try_find_reference(ref_name).map_err(Error::repo)? else {
        return Ok(None);
    };
    let mut platform = reference.log_iter();
    let Some(iter) = platform.rev().map_err(Error::repo)? else {
        return Ok(None);
    };
    let mut lines = iter.filter_map(|line| {
        let line = line.ok()?;
        let time = line.signature.time.seconds;
        Some((line.new_oid, time))
    });
    Ok(select(&mut lines).map(|(id, _)| id))
}
