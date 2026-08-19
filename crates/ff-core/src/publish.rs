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
//! blocked, does the remote already have it, and if not, does the push
//! create the shared copy or replace it. The network itself is somebody
//! else's job — this module hands back a plan and never spawns anything.

use crate::model::{Publish, PublishReport};
use crate::preflight::Preflight;
use crate::{Provenance, Result};

pub struct PublishOptions {
    /// Decide the plan, write nothing, send nothing. The one thing worth
    /// previewing here is which push this would be — creating a shared copy,
    /// replacing one, or putting back one that was deleted are three
    /// different acts wearing one verb.
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
                    // Configured, and either standing somewhere else or gone
                    // entirely. Both are the same push under a different
                    // lease: the tip as last seen, or the empty string, which
                    // is git's spelling for *must not exist* and is what
                    // re-creates a shared copy somebody deleted. Typing
                    // `ff publish` is saying that out loud; when publishing
                    // was a default, this case needed a flag to mean it.
                    Some(tracking) => Publish::Push {
                        remote: remote.clone(),
                        remote_branch: tracking.remote_branch.clone(),
                        lease: tracking.tip.map(|id| id.to_string()).unwrap_or_default(),
                        tip: tip.to_string(),
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
