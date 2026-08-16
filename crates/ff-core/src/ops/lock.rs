//! The one lock the operation log is written under.
//!
//! The two-ref CAS was supposed to be the whole of the write path's
//! exclusion: read the tip, build an operation naming it, and move the refs
//! only if the tip is still what was read. It is not, and the reason is in
//! gix rather than here. `file::Transaction::prepare` reads each reference's
//! existing value *before* it acquires that reference's lock, and compares
//! `MustExistAndMatch` against the value it read. Two writers can therefore
//! both read the same tip, both pass the check, and then take the lock in
//! turn and both apply — so `MustExistAndMatch` guards against a caller
//! holding a stale value, and not against a second writer at all.
//!
//! What that cost was not a failed write but a silent one. The loser's
//! append reported `Committed`, left a reflog line, and moved the pointer;
//! the winner then wrote over it naming the tip from before either of them,
//! and the operation vanished from the log while staying in the reflog and
//! therefore in the id index. Racing captures reproduced it about once in
//! thirty runs, and the id index is where it surfaced, because an id the
//! reflog reaches and the walk does not is exactly the shape a rewind is
//! supposed to make.
//!
//! So fufu excludes its own writers itself, and the CAS stays where it is —
//! now as the second line rather than the first, catching a foreign writer
//! or a stale plan that the lock has nothing to say about.
//!
//! The lock is fufu's own file rather than `refs/fufu/ops.lock`: gix takes
//! that one itself inside the transaction, and a writer holding it would
//! deadlock against its own append. Nothing outside fufu writes the log, so
//! a lock only fufu observes loses nothing.

use std::time::Duration;

use crate::error::{Error, Result};

/// How long a verb waits for another writer before giving up. Long enough to
/// cover an append already in flight — the tree is assembled before the lock
/// is taken, so what is inside it is a few small object writes and two ref
/// edits — and short enough that a stale lock file is noticed rather than
/// waited out.
const VERB_WAIT: Duration = Duration::from_secs(2);

/// Held from the read of the tip until the refs have moved. Dropping it
/// releases the lock; there is nothing to commit, because the lock names a
/// resource rather than staging a new value for one.
pub(crate) struct Guard(#[allow(dead_code)] gix::lock::Marker);

/// What a writer does when someone else holds the lock.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Wait {
    /// A capture: give up at once. Losing means another fufu process is
    /// already recording something, and there are a thousand more captures
    /// coming — the same reasoning that makes a capture never retry its CAS.
    Never,
    /// A verb: wait briefly. A verb is something a person typed, and failing
    /// it because a background capture held the log for a millisecond would
    /// be a worse answer than the wait.
    Briefly,
}

/// Take the log's write lock, or `None` when another writer holds it.
pub(crate) fn acquire(repo: &gix::Repository, wait: Wait) -> Result<Option<Guard>> {
    let dir = repo.common_dir().join("fufu");
    let mode = match wait {
        Wait::Never => gix::lock::acquire::Fail::Immediately,
        Wait::Briefly => gix::lock::acquire::Fail::AfterDurationWithBackoff(VERB_WAIT),
    };
    match gix::lock::Marker::acquire_to_hold_resource(dir.join("oplog"), mode, Some(dir)) {
        Ok(marker) => Ok(Some(Guard(marker))),
        Err(gix::lock::acquire::Error::PermanentlyLocked { .. }) => Ok(None),
        Err(err) => Err(Error::repo(err)),
    }
}
