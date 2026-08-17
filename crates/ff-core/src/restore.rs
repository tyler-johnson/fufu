//! Worktree restore. Load-bearing ordering: resolve the source FIRST (so
//! "now" means the timeline the user just looked at), then take the mandatory
//! pre-restore capture, then write. Writes touch only the worktree — never the
//! index, HEAD, or branches.
//!
//! **A positional argument has exactly one kind**, so the paths go in the
//! position and the source goes behind a flag — and each flag holds exactly
//! one kind in turn:
//!
//! ```text
//! (bare)            the commit under the open change   — a commit
//! --from <rev>      any revision, through the revset   — a commit
//! --at-op <op>      an operation, spelled in letters   — an operation
//! --at <time>       the operation current at a moment  — an operation
//! ```
//!
//! What that arrangement retires is the old `--at`, which took an operation
//! id, *raw hex*, `@{n}`, an age, or a date, and picked between them by
//! shape. Raw hex there was the leak that mattered: hex is how you say
//! *commit* everywhere else in fufu, and one verb quietly accepting it as an
//! operation address is how the two spaces bleed. `@{n}` went with it, since
//! `--at-op @^` says the same thing in the address space that owns the
//! question.

use crate::error::{Error, Result};
use crate::model::{RestoreOrigin, RestoreReport};
use crate::ops::{self, CaptureOutcome, OpLog};
use crate::revset::{Rev, Revset};
use crate::snapshot::{Provenance, TakeOptions};
use crate::worktree;

/// Where a restore pulls from. One variant per flag, one kind per variant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RestoreSource {
    /// The commit under the open change — the bare form, and the everyday
    /// "discard my edits to this file". Both neighbors read this way:
    /// `git restore <path>` and `jj restore <paths>` land in the same place.
    #[default]
    Open,
    /// `--from <rev>` — a revision, through the one revset resolver.
    Rev(String),
    /// `--at-op <op>` — an operation, spelled in letters.
    Op(String),
    /// `--at <time>` — the operation current at that moment.
    Time(String),
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

/// A moment, as a compact age or anything git's own date parser accepts.
///
/// This is the whole of the time grammar now, and it is unambiguous because
/// the position's kind *is* a time: nothing here has to out-guess an id, so
/// `3d` is three days and `123d` is three days too, rather than an object
/// prefix that happens to be all hex.
pub fn parse_time(raw: &str, now: i64) -> Result<i64> {
    let raw = raw.trim();
    if let Some(secs) = parse_compact_age(raw) {
        return Ok(now - secs);
    }
    let time = gix::date::parse(raw, Some(std::time::SystemTime::now())).map_err(|err| {
        Error::coded(
            "usage/bad-restore-target",
            format!("unrecognized time {raw:?}: {err}"),
            vec![
                "ff restore --all --at 2h".into(),
                "ff restore --all --at-op <op>".into(),
            ],
        )
    })?;
    Ok(time.seconds)
}

#[derive(Debug, Clone, Default)]
pub struct RestoreOptions {
    pub source: RestoreSource,
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
                "ff restore <path> --at-op <op>".into(),
            ],
        ));
    }

    let now = opts.now.unwrap_or_else(crate::ops::append::wall_clock);

    // 1. Resolve first: an age and "the newest" refer to the timeline as the
    //    user saw it, before the pre-restore capture moves anything.
    let (origin, source_tree) = resolve(repo, &opts.source, now)?;

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

    // 3. Diff fresh capture → source — never worktree → source, where
    //    untracked files would read as deletions.
    let select = |path: &str| opts.all || path_selected(path, &opts.paths);
    let transition = worktree::apply_tree_transition(repo, fresh_tree, source_tree, &select)?;

    Ok(RestoreReport {
        origin,
        restored: transition.written,
        deleted: transition.deleted,
        skipped_gitlinks: transition.skipped_gitlinks,
        pre_op,
    })
}

/// Whether `path` is selected by any of `selectors` — a file path or a
/// directory prefix.
pub(crate) fn path_selected(path: &str, selectors: &[String]) -> bool {
    selectors.iter().any(|sel| {
        let sel = sel.trim_end_matches('/');
        path == sel || path.starts_with(&format!("{sel}/"))
    })
}

/// Resolve a source to the tree it offers, and to the row describing it.
///
/// Every operation-space exit is identity-guarded by the resolver it goes
/// through: [`OpLog::resolve`] refuses hex outright and confirms every
/// candidate is an operation commit — not merely a commit bearing the fufu
/// identity, which a *record* commit does too, and restoring from one would
/// wipe the working tree and write three metadata files in its place.
fn resolve(
    repo: &gix::Repository,
    source: &RestoreSource,
    now: i64,
) -> Result<(RestoreOrigin, gix::ObjectId)> {
    match source {
        RestoreSource::Open => {
            let head = repo.head_commit().map_err(|_| unborn())?;
            commit_origin(repo, head.id().detach())
        }
        RestoreSource::Rev(raw) => {
            let point = Revset::parse(raw)?.point(repo)?;
            match point.rev {
                Rev::Commit(id) => commit_origin(repo, id.object_id()),
                // Refused rather than answered with a no-op: `@` is the open
                // change, which is where the files already are, so a restore
                // from it can only be a spelling somebody did not mean.
                Rev::Open => Err(Error::coded(
                    "target/unresolvable",
                    "`@` is the open change: restoring from it would put the files back \
                     where they already are. The commit under it is `HEAD`",
                    vec![
                        "ff restore <path> --from HEAD".into(),
                        "ff restore <path>".into(),
                    ],
                )),
            }
        }
        RestoreSource::Op(spec) => {
            let id = OpLog::open(repo)?.resolve(spec)?;
            op_origin(repo, id)
        }
        RestoreSource::Time(raw) => {
            let at = parse_time(raw, now)?;
            let log = OpLog::open(repo)?;
            // The operation current at that moment: the newest one at or
            // before it, over the whole log rather than one branch's slice of
            // it, because there is one log and "what was true then" is not a
            // question about which branch you happen to stand on now.
            for op in log.iter() {
                let op = op?;
                if op.time() <= at {
                    return op_origin(repo, op.id());
                }
            }
            Err(Error::coded(
                "op/not-found",
                format!("no operation on the log at or before {raw}"),
                vec!["ff op log".into()],
            ))
        }
    }
}

fn commit_origin(
    repo: &gix::Repository,
    id: gix::ObjectId,
) -> Result<(RestoreOrigin, gix::ObjectId)> {
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let tree = commit.tree_id().map_err(Error::repo)?.detach();
    let subject = commit
        .message()
        .map(|m| m.summary().to_string())
        .unwrap_or_default();
    let time = commit.time().map_err(Error::repo)?.seconds;
    Ok((
        RestoreOrigin {
            space: "commit".into(),
            id: id.to_string(),
            // Seven characters, plain: commit shas get no prefix
            // highlighting, and 7 is effectively always unique at this scale.
            short_id: id.to_string().chars().take(7).collect(),
            subject,
            time,
        },
        tree,
    ))
}

fn op_origin(repo: &gix::Repository, id: ops::OpId) -> Result<(RestoreOrigin, gix::ObjectId)> {
    let op = OpLog::open(repo)?.get(id)?;
    let hex = id.hex();
    let len = ops::index::prefix_lens(repo, std::slice::from_ref(&hex))?
        .get(&hex)
        .copied()
        .unwrap_or(8)
        .max(4);
    Ok((
        RestoreOrigin {
            space: "operation".into(),
            id: hex,
            short_id: id.short(len),
            subject: op.summary().to_string(),
            time: op.time(),
        },
        op.tree(),
    ))
}

fn unborn() -> Error {
    Error::coded(
        "op/not-found",
        "this branch has no commits yet, so there is nothing under the open change to \
         restore from",
        vec!["ff restore <path> --at-op <op>".into(), "ff op log".into()],
    )
}
