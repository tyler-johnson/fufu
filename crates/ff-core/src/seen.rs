//! Where fufu last showed you the shared copy standing, and why that is a
//! ref of its own.
//!
//! The lease `ff push` sends is an expected value: *the shared copy stands
//! here, and if it does not, refuse*. That value is only worth something as
//! the tip you last looked at. The tracking ref is not that. Any fetch moves
//! it — an editor's background one, `ff git fetch`, `ff pull --dry-run`,
//! whose fetch writes the tracking refs on purpose, the fetch lane that
//! rides any verb on its cadence — and a lease read off
//! it afterwards names a tip nobody looked at, which git then honors,
//! and a teammate's push is gone. So the lease is fufu's own record: the
//! tip of the shared copy a foreground verb last put in front of you, and
//! nothing that runs without a report writes it. No fetch does, foreground
//! or background, and `ff pull --dry-run` does not either, since its
//! promise is that it writes nothing of fufu's.
//!
//! Who writes it: [`push::record`](crate::push::record), to the tip it
//! sent; `ff pull`'s real run, to the tip it read for every branch whose
//! remote axis it reported, whatever the outcome; `ff switch` minting a
//! branch that continues a remote's, to the tip it minted at; and `ff
//! clone`, to the tip it checked out. `ff branch -d --shared` deletes it
//! with the copy's other traces.
//!
//! Like [`crate::published`] it is a ref outside `TRACKED_PREFIXES` and
//! outside undo, for the same reason: the wire does not step back. An
//! undone pull does not un-see the tip it reported, and a push after that
//! undo is the person's decision under an honest lease. What you saw stays
//! seen.
//!
//! Failure points the safe way: an unreadable or absent record answers
//! *none*, and push treats none as a copy it cannot vouch for.

use crate::error::Result;

/// Where the last-seen tip of one branch's shared copy is remembered.
/// Outside `TRACKED_PREFIXES` on purpose: fufu's own note to itself, not
/// repository state it guards against foreign motion.
pub const SEEN_PREFIX: &str = "refs/fufu/seen/";

fn ref_name(branch: &str) -> String {
    format!("{SEEN_PREFIX}{branch}")
}

/// The tip a foreground verb last showed the shared copy of `branch`
/// standing at, or `None` when fufu has no record of one.
pub fn last_seen(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    Ok(crate::refs::ref_target(repo, &ref_name(branch)).unwrap_or(None))
}

/// Remember that the shared copy of `branch` was shown standing at `to`.
pub(crate) fn mark(
    repo: &gix::Repository,
    branch: &str,
    to: gix::ObjectId,
    now: i64,
) -> Result<()> {
    crate::refs::write_ref(
        repo,
        &ref_name(branch),
        to,
        gix::refs::transaction::PreviousValue::Any,
        now,
        "seen: the shared copy stood here",
    )
}

/// Mark `branch`'s shared copy seen at the tip its tracking ref holds now,
/// for the verbs that put that tip in front of the person as their
/// starting point: a clone's checkout and a switch that mints a branch
/// continuing a remote's. Nothing to mark when the branch has no copy or
/// the tracking ref is absent.
pub fn mark_seen_from_tracking(repo: &gix::Repository, branch: &str, now: i64) -> Result<()> {
    let Some(pull_ref) = crate::futures::remote_for(repo, branch)? else {
        return Ok(());
    };
    if pull_ref.tip.is_empty() {
        return Ok(());
    }
    let tip = gix::ObjectId::from_hex(pull_ref.tip.as_bytes()).map_err(crate::Error::repo)?;
    mark(repo, branch, tip, now)
}

/// Forget the record for `branch`, tolerant of there being none: the
/// shared copy is gone, and there is nothing left for it to name.
pub(crate) fn forget(repo: &gix::Repository, branch: &str, now: i64) -> Result<()> {
    let name = ref_name(branch);
    if let Some(tip) = crate::refs::ref_target(repo, &name)? {
        crate::refs::delete_ref(repo, &name, tip, now)?;
    }
    Ok(())
}
