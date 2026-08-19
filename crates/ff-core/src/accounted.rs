//! Which of these commits can fufu's own log account for. A commit is
//! accounted for when some recorded operation names it as the `old` side of
//! a rewrite, or when a replay dropped it as empty — both things fufu did,
//! and anything else is somebody else's work. This is the first reader of
//! the rewrite map outside the tests, and it still does not need an index:
//! a handful of shas, once per sync, against a walk bounded by the oldest
//! queried commit. Every failure mode fails toward *not* accounted for — an
//! unreadable sha, a trimmed log, an aggressive floor all mean the caller
//! takes the unaccounted path, which is the safe one.

use std::collections::HashSet;

use crate::error::Result;

/// The members of `shas` that fufu's operation log accounts for: each one
/// recorded as the `old` side of a rewrite, or dropped by a replay as empty.
/// The answer never names a sha that was not queried, and a sha the log
/// cannot account for simply stays out of the set.
pub fn accounted_for(repo: &gix::Repository, shas: &[String]) -> Result<HashSet<String>> {
    if shas.is_empty() {
        return Ok(HashSet::new());
    }

    let wanted: HashSet<&str> = shas.iter().map(String::as_str).collect();
    // An operation cannot account for a commit that did not exist when it
    // ran, so the walk stops at the oldest queried commit. A sha we cannot
    // parse, find, or date drops the floor away rather than risk a wrong no.
    let mut floor: Option<i64> = Some(i64::MAX);
    for sha in &wanted {
        let seconds = gix::ObjectId::from_hex(sha.as_bytes())
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
            .and_then(|commit| commit.time().ok())
            .map(|time| time.seconds);
        floor = match seconds {
            Some(s) => floor.map(|f| f.min(s)),
            None => None,
        };
    }
    let floor = floor.unwrap_or(i64::MIN);

    let log = crate::ops::OpLog::open(repo)?;
    let mut found: HashSet<String> = HashSet::new();
    for op in log.iter_verbs() {
        let op = op?;
        let Some(record) = op.record()? else {
            // A capture carries no record and cannot account for anything.
            continue;
        };
        // Record this operation's matches before checking either stop
        // condition below — the op sitting exactly at the floor still gets
        // read.
        for rewrite in &record.rewrites {
            if wanted.contains(rewrite.old.as_str()) {
                found.insert(rewrite.old.clone());
            }
        }
        for dropped in &record.dropped {
            if wanted.contains(dropped.old.as_str()) {
                found.insert(dropped.old.clone());
            }
        }
        if found.len() >= wanted.len() || op.time() < floor {
            break;
        }
    }
    Ok(found)
}
