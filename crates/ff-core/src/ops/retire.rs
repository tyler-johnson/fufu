//! Retiring a branch of the log.
//!
//! An undo steps the pointer back rather than appending, so what it steps off
//! stays addressable — the ops ref's own reflog records where the pointer has
//! stood, and every position it names seeds the resolution domain. That is
//! what keeps `ff redo` able to walk forward and `ff op restore` able to
//! accept an abandoned id.
//!
//! `ff op abandon` is how that stops. It marks the reflog positions that
//! reach an operation, and the domain skips a marked seed: the operations
//! themselves are untouched objects that `ff op show` still reads, they
//! simply stop being somewhere the log can walk to, and trim ages them out on
//! the same window as everything else.
//!
//! Marking rather than pruning is the whole design. Editing git's own reflog
//! under a live ref means deleting and recreating it, and a crash in that gap
//! takes the operation log with it — for a verb whose entire job is to
//! *forget* something. A marker ref only ever adds.

use std::collections::BTreeSet;

use crate::error::{Error, Result};
use crate::ops::id::OpId;
use crate::refs;

/// One ref per retired seed, named by the seed's own hex. Listable by prefix,
/// cheap to read, and ordinary to every other tool.
pub const RETIRED_PREFIX: &str = "refs/fufu/retired/";

/// Every reflog position that has been retired. Read once per domain build.
pub fn retired(repo: &gix::Repository) -> Result<BTreeSet<gix::ObjectId>> {
    let mut out = BTreeSet::new();
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed(RETIRED_PREFIX).map_err(Error::repo)?;
    for reference in iter {
        let Ok(reference) = reference else { continue };
        if let Some(id) = reference.target().try_id() {
            out.insert(id.to_owned());
        }
    }
    Ok(out)
}

/// Which reflog positions of the ops ref reach `target`, and so are the ones
/// abandoning it must retire.
///
/// Reachability is a backward walk, which is the only direction the log has,
/// and it is bounded by time: operations run in committer order along the
/// chain, so a walk that has gone older than the target has passed it. That
/// turns an unbounded ancestry question into a scan of the seeds that could
/// plausibly answer yes.
pub fn seeds_reaching(
    repo: &gix::Repository,
    target: OpId,
    floor_time: i64,
) -> Result<Vec<gix::ObjectId>> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<gix::ObjectId> = BTreeSet::new();
    for line in refs::read_ref_log(repo, crate::ops::OPS_REF)? {
        if !seen.insert(line.new) {
            continue;
        }
        if reaches(repo, line.new, target, floor_time)? {
            out.push(line.new);
        }
    }
    Ok(out)
}

fn reaches(
    repo: &gix::Repository,
    from: gix::ObjectId,
    target: OpId,
    floor_time: i64,
) -> Result<bool> {
    let mut cur = Some(from);
    while let Some(id) = cur {
        if id == target.object_id() {
            return Ok(true);
        }
        let Ok(op) = crate::ops::walk::decode(repo, id) else {
            return Ok(false);
        };
        if op.time() < floor_time {
            return Ok(false); // walked past it: this seed does not reach it
        }
        cur = op.prev().map(|p| p.object_id());
    }
    Ok(false)
}

/// Mark seeds retired. Idempotent: a seed already marked is left alone.
pub fn retire(repo: &gix::Repository, seeds: &[gix::ObjectId], now: i64) -> Result<usize> {
    let mut marked = 0;
    for seed in seeds {
        let name = format!("{RETIRED_PREFIX}{seed}");
        if refs::ref_target(repo, &name)?.is_some() {
            continue;
        }
        refs::write_ref(
            repo,
            &name,
            *seed,
            gix::refs::transaction::PreviousValue::MustNotExist,
            now,
            "fufu: abandoned this branch of the log",
        )?;
        marked += 1;
    }
    Ok(marked)
}
