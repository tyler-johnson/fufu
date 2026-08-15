//! Worktree restore from the timeline. Load-bearing ordering: resolve the
//! target FIRST (so `@{1}` and "newest" mean the timeline the user just
//! looked at), then take the mandatory pre-restore snapshot, then write.
//! Writes touch only the worktree — never the index, HEAD, or branches.

use gix::prelude::ObjectIdExt;

use crate::error::{Error, Result};
use crate::model::{RestoreReport, SnapEntry, SnapOutcome};
use crate::snapshot::chain;
use crate::snapshot::{Provenance, TakeOptions};
use crate::worktree;

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

/// Parse the target grammar: nothing, a hex or letters-spelled id prefix,
/// `@{n}`, a compact duration (`90s`/`15m`/`2h`/`3d`/`1w`), or a git-style
/// date string.
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
    // Letters-spelled ids win over date words: `noon` and `tomorrow` are
    // all-alphabet and parse as id prefixes — accepted shadowing (DESIGN.md).
    if raw.len() >= 4
        && raw.len() <= 40
        && crate::snapid::is_encoded(raw)
        && let Some(hex) = crate::snapid::decode(raw)
    {
        return Ok(RestoreTarget::Id(hex));
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
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to restore into",
            vec![],
        ));
    }
    if !opts.all && opts.paths.is_empty() {
        return Err(Error::coded(
            "restore/nothing-selected",
            "nothing selected: pass paths or --all",
            vec![
                "ff restore --all".into(),
                "ff restore <path> --at <id>".into(),
            ],
        ));
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

    // 3. Diff fresh snapshot → target — never worktree → target, where
    //    untracked files would read as deletions.
    let target_tree = repo
        .find_commit(target_id)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();
    let select = |path: &str| opts.all || path_selected(path, &opts.paths);
    let transition = worktree::apply_tree_transition(repo, fresh_tree, target_tree, &select)?;

    Ok(RestoreReport {
        target: target_entry,
        restored: transition.written,
        deleted: transition.deleted,
        skipped_gitlinks: transition.skipped_gitlinks,
        pre_snapshot,
    })
}

fn path_selected(path: &str, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| {
        let sel = sel.trim_end_matches('/');
        path == sel || path.starts_with(&format!("{sel}/"))
    })
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
        RestoreTarget::Newest => chain::tip(repo, &snap_ref)?.ok_or_else(|| {
            Error::msg(format!(
                "no snapshots on {chain_name} yet — nothing to restore"
            ))
        })?,
        RestoreTarget::Id(prefix) => {
            // Try the materialized index first; fall back to the walk when
            // the index can't be consulted. Either way the id below still
            // passes through the identity guard at the end of this function,
            // so a stale index can only ever offer a candidate that then
            // fails the guard — never cause a wrong restore.
            let mut candidates = match crate::idindex::prefix_matches(repo, chain_name, prefix)? {
                Some(found) => found,
                None => {
                    let mut candidates = Vec::new();
                    for r in [snap_ref.clone(), chain::trash_ref(chain_name)] {
                        if let Some(tip) = chain::tip(repo, &r)? {
                            collect_prefix_matches(repo, tip, prefix, &mut candidates)?;
                        }
                    }
                    candidates
                }
            };
            // Index candidates come back in sorted hex order rather than
            // newest-first walk order, so an ambiguous-prefix error may list
            // ids in a different order than before. The message text is
            // unchanged; only the order of the list it prints can differ.
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
