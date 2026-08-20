//! `ff publish`: the outgoing half of lining up.
//!
//! Everything `ff sync` does is undoable — fetch, replay, re-parent, all of
//! it recorded and all of it reachable from `ff undo`. Publishing is the one
//! act in the pair that leaves the machine, and no operation log can reach
//! across the wire to take it back. So it is its own verb, typed on purpose,
//! rather than a default riding along inside a verb whose whole promise is
//! reversibility.
//!
//! Publish does not fetch. That is not an omission — the lease wants the
//! tracking ref *as you last saw it*, and the last thing that moved it was
//! the last fetch. Going to the network first to refresh that value would
//! ask git to protect you against a change you just accepted sight unseen.
//! `ff sync` is how you look.
//!
//! What it decides is small: is there anywhere to send this, is the exit
//! blocked, does the remote already have it, and if not, what the push does
//! to the shared copy. The network itself is somebody else's job — this
//! module hands back a plan and never spawns anything.
//!
//! And then, once somebody else has made the call, it records it. That is
//! the whole of what publish writes, and [`record`] is a second entry point
//! rather than a step inside [`publish`] because the push happens between
//! them: what it writes is a fact, written after the wire agreed, and there
//! is no local ref to diff a write-ahead claim against. If either write is
//! lost after a successful push, the next sync reads the remote as theirs
//! and replays, which never loses work.
//!
//! Two marks, not one, and [`crate::published`] is where the reason lives:
//! the note is the record a person reads and is rewound by `ff undo` with
//! everything else above the landing; the pointer is the answer sync needs
//! and is the one thing undo must not step back, because undo cannot step
//! back the wire.

use crate::model::{Publish, PublishReport, PushShape};
use crate::ops::record::{OpRecord, Published, observe_refs};
use crate::ops::{OpId, OpKind};
use crate::preflight::Preflight;
use crate::{Error, Provenance, Result};

pub struct PublishOptions {
    /// Decide the plan, write nothing, send nothing. The one thing worth
    /// previewing here is which push this would be — creating a shared copy,
    /// replacing one, putting back one that was deleted, and rolling one
    /// back are four different acts wearing one verb.
    pub dry_run: bool,
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

pub fn publish(
    repo: &gix::Repository,
    pre: &Preflight,
    opts: PublishOptions,
    prov: &Provenance,
) -> Result<(PublishReport, Option<crate::ops::verb::VerbContext>)> {
    // Capture first, like every verb. Publish changes nothing locally, so it
    // records no operation of its own — but the tree is snapshotted and
    // foreign motion is reconciled before anything leaves, which is the
    // point of the floor. A dry run reads and writes nothing, so it takes
    // nothing: same rule as `ff trim -n`.
    let ctx = if opts.dry_run {
        None
    } else {
        Some(crate::ops::verb::begin_verb(repo, prov, opts.now)?)
    };

    let publish = if crate::held::of(repo, &pre.branch)?.is_some() {
        // The exits-blocked discipline: a held rewrite means the branch's
        // commits are not what they will be, and sending them would publish
        // a state fufu is about to rewrite out from under.
        Publish::Blocked
    } else {
        match pre.remote.as_ref() {
            None => Publish::NoRemote,
            Some(remote) => {
                let tip = crate::preflight::branch_tip(repo, &pre.branch)?;
                match pre.tracking.as_ref() {
                    // No upstream at all: the push is what creates one and
                    // starts tracking it.
                    None => Publish::Create {
                        remote: remote.clone(),
                        remote_branch: pre.branch.clone(),
                        tip: tip.to_string(),
                    },
                    Some(tracking) if tracking.tip == Some(tip) => Publish::UpToDate,
                    // Configured, and either standing somewhere else or not
                    // there at all. Every one of these is the same push under
                    // a different lease: the tip as last seen, or the empty
                    // string, which is git's spelling for *must not exist*.
                    // Typing `ff publish` is saying that out loud; when
                    // publishing was a default, this case needed a flag to
                    // mean it. Only the sentence afterwards differs, and
                    // `shape` is what tells the four apart.
                    Some(tracking) => Publish::Push {
                        remote: remote.clone(),
                        remote_branch: tracking.remote_branch.clone(),
                        lease: tracking.tip.map(|id| id.to_string()).unwrap_or_default(),
                        tip: tip.to_string(),
                        shape: shape(repo, pre, tracking, tip)?,
                    },
                }
            }
        }
    };

    Ok((
        PublishReport {
            branch: pre.branch.clone(),
            publish,
            dry_run: opts.dry_run,
        },
        ctx,
    ))
}

/// Which of the four pushes this is.
///
/// The absent-tracking-ref half used to be one answer — *somebody deleted
/// the shared copy* — and in a fresh clone of an empty remote that is a loss
/// report about a thing that never existed. `git clone` writes
/// `branch.<n>.merge` and creates no `refs/remotes/*`, so the configured-but-
/// absent shape is exactly what a brand new clone wears. Evidence is what
/// separates them, checked cheapest first: any ref under
/// `refs/remotes/<remote>/` (a clone of a non-empty remote always has some),
/// then the log's own memory of a push.
fn shape(
    repo: &gix::Repository,
    pre: &Preflight,
    tracking: &crate::preflight::Tracking,
    tip: gix::ObjectId,
) -> Result<PushShape> {
    let Some(seen) = tracking.tip else {
        return Ok(if ever_copied(repo, pre)? {
            PushShape::Recreate
        } else {
            PushShape::First
        });
    };
    // Your tip is an ancestor of what the remote holds: this push does not
    // send commits, it takes them off the shared copy. Saying "published" of
    // that would name the opposite act.
    let ancestor = repo
        .merge_bases_many(tip, &[seen])
        .map_err(Error::repo)?
        .into_iter()
        .any(|base| base.detach() == tip);
    Ok(if ancestor {
        PushShape::Retract
    } else {
        PushShape::Replace
    })
}

/// Whether anything says this branch ever had a copy on the remote.
pub(crate) fn ever_copied(repo: &gix::Repository, pre: &Preflight) -> Result<bool> {
    if let Some(remote) = pre.remote.as_ref()
        && crate::refs::any_remote_ref(repo, remote)?
    {
        return Ok(true);
    }
    crate::published::ever_published(repo, &pre.branch)
}

/// Record the push, after it has left the machine.
///
/// A note rather than an op, and the kind is the whole argument: a note
/// marks something that happened rather than something that was done, so
/// `ff undo` steps over it and `ff op revert` refuses it. That is exactly a
/// push — the local repository is unchanged, and there is nothing here to
/// put back. One deviation from the notes that ship today: `init` and `trim`
/// reference nothing and pin nothing, and this one names a sha, so `to` is
/// pinned rather than left for gc to eat out from under the row.
///
/// The pointer goes down after the note, and it is the half the readers use.
/// See [`crate::published`] for why the row alone would not do.
pub fn record(
    repo: &gix::Repository,
    pre: &Preflight,
    report: &PublishReport,
    ctx: &crate::ops::verb::VerbContext,
    prov: &Provenance,
) -> Result<Option<OpId>> {
    let (remote, remote_branch, from, to) = match &report.publish {
        Publish::Create {
            remote,
            remote_branch,
            tip,
        } => (remote, remote_branch, None, tip),
        Publish::Push {
            remote,
            remote_branch,
            lease,
            tip,
            ..
        } => (
            remote,
            remote_branch,
            (!lease.is_empty()).then(|| lease.clone()),
            tip,
        ),
        // Nothing left the machine, so there is nothing to remember.
        Publish::NoRemote | Publish::Blocked | Publish::UpToDate => return Ok(None),
    };

    let mut record = OpRecord::new(
        "publish",
        format!("published {} to {remote}/{remote_branch}", pre.branch),
        ctx.now,
    );
    record.published = Some(Published {
        remote: remote.clone(),
        remote_branch: remote_branch.clone(),
        from,
        to: to.clone(),
    });
    // The one pin. `to` is the whole answer this row exists to give, and a
    // row naming a sha gc has since collected would be a dangling claim.
    let pins: Vec<gix::ObjectId> = gix::ObjectId::from_hex(to.as_bytes()).into_iter().collect();
    let head = crate::head::head_state(repo)?;
    let id = crate::ops::verb::append_op(
        repo,
        OpKind::Note,
        crate::ops::verb::VerbOp {
            record,
            // Publish moves no local ref, so the planned state is the state:
            // observing it here can only ever write back what the capture
            // already agreed to.
            planned: observe_refs(repo)?,
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            // The branch preflight read, not the chain name — this row is
            // looked up by `published::published_tip` along exactly that
            // branch's pointer, and the two must be one name.
            branch: pre.branch.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        ctx.now,
    )?;
    if let Ok(oid) = gix::ObjectId::from_hex(to.as_bytes()) {
        crate::published::mark(repo, &pre.branch, oid, ctx.now)?;
    }
    Ok(Some(id))
}
