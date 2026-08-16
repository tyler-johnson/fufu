//! Worktree restore from the timeline. Load-bearing ordering: resolve the
//! target FIRST (so `@{1}` and "newest" mean the timeline the user just looked
//! at), then take the mandatory pre-restore capture, then write. Writes touch
//! only the worktree — never the index, HEAD, or branches.
//!
//! Targets are operations now rather than snapshots, and that is a widening
//! rather than a change of subject: a snapshot is what an operation carries,
//! so every operation has a tree to restore, and the ids `ff evolog` prints
//! are the same ids they always were.

use crate::error::{Error, Result};
use crate::model::{RestoreReport, SnapEntry};
use crate::ops::{self, BRANCH_PREFIX, CaptureOutcome, OpLog};
use crate::snapshot::chain;
use crate::snapshot::{Provenance, TakeOptions};
use crate::worktree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreTarget {
    /// The newest operation on the branch (the default).
    Newest,
    /// An operation id prefix, as raw hex.
    Id(String),
    /// `@{n}` — the reflog entry n steps back.
    Back(usize),
    /// The branch pointer as of a moment in time (`@{<date>}` semantics).
    AtTime(i64),
}

/// Git's own shortest-accepted object prefix. Borrowed rather than restated:
/// it is what separates an id from a duration below.
const MIN_HEX_LEN: usize = gix::hash::Prefix::MIN_HEX_LEN;

/// Parse the target grammar: nothing, a hex or letters-spelled id prefix,
/// `@{n}`, a compact duration (`90s`/`15m`/`2h`/`3d`/`1w`), or a git-style
/// date string. Ids are tried before durations — see the note inline.
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
    // Hex before ages: `d` is both a duration unit and a hex digit, so `123d`
    // is a legal object prefix and must resolve as one. Ages survive because
    // git's own four-character prefix minimum excludes them — `3d` and `10d`
    // are too short to be a prefix, so they still read as durations.
    if raw.len() >= MIN_HEX_LEN && raw.len() <= 40 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(RestoreTarget::Id(raw.to_ascii_lowercase()));
    }
    if let Some(secs) = parse_compact_age(raw) {
        return Ok(RestoreTarget::AtTime(now - secs));
    }
    // Letters-spelled ids win over date words: `noon` and `tomorrow` are
    // all-alphabet and parse as id prefixes — accepted shadowing (DESIGN.md).
    if raw.len() >= MIN_HEX_LEN
        && raw.len() <= 40
        && crate::snapid::is_encoded(raw)
        && let Some(hex) = crate::snapid::decode(raw)
    {
        return Ok(RestoreTarget::Id(hex));
    }
    let time = gix::date::parse(raw, Some(std::time::SystemTime::now())).map_err(|err| {
        Error::coded(
            "usage/bad-restore-target",
            format!("unrecognized restore target {raw:?}: {err}"),
            vec!["ff evolog".into(), "ff restore --at 2h".into()],
        )
    })?;
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

/// Resolve, capture, write. `prov` names the mandatory pre-restore capture
/// (`pre: ff restore …`); if that capture is contended the restore aborts.
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
    let branch = chain::chain_name(&head);

    let now = opts.now.unwrap_or_else(crate::ops::append::wall_clock);

    // 1. Resolve first: "newest" and @{n} refer to the timeline as the user
    //    saw it, before the pre-restore capture moves the pointer.
    let target = parse_target(opts.target.as_deref(), now)?;
    let target_id = resolve(repo, &target, &branch)?;
    let target_entry = entry_of(repo, target_id)?;

    // 2. Mandatory pre-restore capture: the state being overwritten must be on
    //    the log before a single byte moves.
    let pre = ops::capture_with(
        repo,
        prov,
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )?;
    let log = OpLog::open(repo)?;
    let (fresh_tree, pre_op) = match &pre {
        CaptureOutcome::Created { id, .. } => (log.get(*id)?.tree(), Some(id.to_string())),
        CaptureOutcome::NoOp { tip: Some(tip), .. } => (log.get(*tip)?.tree(), None),
        CaptureOutcome::NoOp { tip: None, .. } => (
            repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
            None,
        ),
        CaptureOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "a concurrent fufu capture is in progress; restore aborted (nothing was written)",
                vec![],
            ));
        }
    };

    // 3. Diff fresh capture → target — never worktree → target, where
    //    untracked files would read as deletions.
    let target_tree = log.get(ops::OpId::new(target_id))?.tree();
    let select = |path: &str| opts.all || path_selected(path, &opts.paths);
    let transition = worktree::apply_tree_transition(repo, fresh_tree, target_tree, &select)?;

    Ok(RestoreReport {
        target: target_entry,
        restored: transition.written,
        deleted: transition.deleted,
        skipped_gitlinks: transition.skipped_gitlinks,
        pre_op,
    })
}

fn path_selected(path: &str, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| {
        let sel = sel.trim_end_matches('/');
        path == sel || path.starts_with(&format!("{sel}/"))
    })
}

fn entry_of(repo: &gix::Repository, id: gix::ObjectId) -> Result<SnapEntry> {
    let mut entry = crate::evolog::snap_entry(repo, id)?
        .ok_or_else(|| not_an_op(id))?
        .entry;
    use gix::prelude::ObjectIdExt;
    entry.short_id = id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string());
    Ok(entry)
}

/// Resolve a target against the branch. Every exit is identity-guarded: the
/// resolved commit must be a fufu *operation* or the restore refuses.
///
/// The guard is [`ops::is_op_commit`] and not "does it bear the fufu
/// identity", which is what the old one asked. A record commit bears the
/// identity too, and restoring from one would wipe the working tree and write
/// three metadata files in its place.
fn resolve(repo: &gix::Repository, target: &RestoreTarget, branch: &str) -> Result<gix::ObjectId> {
    let pointer = format!("{BRANCH_PREFIX}{branch}");
    let id = match target {
        RestoreTarget::Newest => crate::refs::ref_target(repo, &pointer)?.ok_or_else(|| {
            Error::coded(
                "op/not-found",
                format!("no operations on {branch} yet — nothing to restore"),
                vec!["ff evolog".into()],
            )
        })?,
        RestoreTarget::Id(prefix) => {
            // The index is a cache, so every candidate it offers is checked
            // against the object store below. A stale entry can only ever
            // produce a candidate that then fails the guard — never a wrong
            // restore.
            let mut candidates = Vec::new();
            for candidate in ops::index::prefix_matches(repo, prefix)? {
                if ops::is_op_commit(repo, candidate)? {
                    candidates.push(candidate);
                }
            }
            candidates.sort_unstable();
            candidates.dedup();
            match candidates.as_slice() {
                [] => {
                    return Err(Error::coded(
                        "op/not-found",
                        format!("no operation matches {prefix}"),
                        vec!["ff evolog".into(), "ff log --ops".into()],
                    ));
                }
                [one] => *one,
                many => {
                    let list: Vec<String> = many
                        .iter()
                        .map(|id| ops::OpId::new(*id).short(12))
                        .collect();
                    return Err(Error::coded(
                        "op/ambiguous",
                        format!(
                            "{prefix} matches {} operations: {}",
                            many.len(),
                            list.join(", ")
                        ),
                        vec!["ff evolog".into()],
                    ));
                }
            }
        }
        RestoreTarget::Back(n) => {
            reflog_entry(repo, &pointer, |lines| lines.nth(*n))?.ok_or_else(|| {
                Error::coded(
                    "op/not-found",
                    format!("@{{{n}}}: not that many operations on {branch}"),
                    vec!["ff evolog".into()],
                )
            })?
        }
        RestoreTarget::AtTime(t) => reflog_entry(repo, &pointer, |lines| {
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
            Error::coded(
                "op/not-found",
                format!("no operation on {branch} at or before that time"),
                vec!["ff evolog".into()],
            )
        })?,
    };
    if !ops::is_op_commit(repo, id)? {
        return Err(not_an_op(id));
    }
    Ok(id)
}

fn not_an_op(id: gix::ObjectId) -> Error {
    Error::coded(
        "op/not-found",
        format!("{id} is not a fufu operation; refusing to restore from it"),
        vec!["ff evolog".into()],
    )
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
