//! What this repository has sent, and where the memory of it lives.
//!
//! `git push` moves `refs/remotes/<remote>/<branch>` as a side effect, and
//! the ref table deliberately excludes remotes — their churn stays silent —
//! so without a memory of its own a push is a thing fufu did and cannot see.
//! The question both readers ask is one question: *is the tracking tip a tip
//! this branch published?* If the remote stands exactly where you last sent
//! it, everything reachable from it that you now lack was yours when you
//! sent it, which is why one sha answers it and a set is not needed.
//!
//! **The memory is a ref, not a row on the log**, and the reason is undo.
//! `ff undo` is a pointer move: `refs/fufu/ops` steps back to the landing
//! and everything above it leaves the chain, publish notes included. That is
//! correct for the log — at the landing the push had not happened yet — and
//! it is exactly wrong for this question, because the one thing undo cannot
//! step back is the wire. A repository that publishes, undoes, and asks
//! "where did I leave the remote?" would be told *nowhere* by a log rewound
//! past its own answer, and fufu would go right back to replaying the undone
//! commits in. So the push leaves two marks: the note on the log, which is
//! the record a person reads in `ff op log` and is rewound with everything
//! else, and this pointer, which is not.
//!
//! Both are written by [`publish::record`](crate::publish::record), in that
//! order, and nothing else writes either.
//!
//! Failure points the same direction [`accounted_for`](crate::accounted_for)
//! does — an unreadable or absent pointer answers *no*, which routes the
//! caller to replay, and replay never loses work.

use crate::error::Result;

/// Where the last-published tip of one branch is remembered. Outside
/// `TRACKED_PREFIXES` on purpose: this is not repository state fufu is
/// guarding against foreign motion, it is fufu's own note to itself, and
/// reconciliation flagging it would report the push as somebody else's edit.
pub const PUBLISHED_PREFIX: &str = "refs/fufu/published/";

fn ref_name(branch: &str) -> String {
    format!("{PUBLISHED_PREFIX}{branch}")
}

/// Does the shared copy of `branch` stand exactly where this repository last
/// sent it?
pub fn published_tip(repo: &gix::Repository, branch: &str, sha: &str) -> Result<bool> {
    Ok(last_published(repo, branch)?.is_some_and(|to| to.to_string() == sha))
}

/// Has this repository ever sent `branch` anywhere?
///
/// The evidence half of "gone": a tracking ref that is configured and absent
/// means the shared copy was deleted only if there is reason to believe one
/// existed. With no reason, it was never created — which is what a clone of
/// an empty remote looks like, and what used to be reported as a loss.
pub fn ever_published(repo: &gix::Repository, branch: &str) -> Result<bool> {
    Ok(last_published(repo, branch)?.is_some())
}

/// The tip this repository last left the shared copy of `branch` standing at.
pub fn last_published(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    Ok(crate::refs::ref_target(repo, &ref_name(branch)).unwrap_or(None))
}

/// Remember that `branch` was left standing at `to`. Written after the push,
/// like the note beside it: there is no local ref to diff a write-ahead
/// claim against, so an append-before would be a claim nothing could
/// falsify. If this write is lost after a successful push, the next sync
/// reads the shared copy as somebody else's and replays onto it, which never
/// loses work.
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
        "publish: left the shared copy here",
    )
}
