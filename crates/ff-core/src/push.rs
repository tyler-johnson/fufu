//! `ff push`: the outgoing half of lining up.
//!
//! Everything `ff pull` does is undoable — fetch, replay, re-parent, all of
//! it recorded and all of it reachable from `ff undo`. Pushing is the one
//! act in the pair that leaves the machine, and no operation log can reach
//! across the wire to take it back. So it is its own verb, typed on purpose,
//! rather than a default riding along inside a verb whose whole promise is
//! reversibility.
//!
//! Push does not fetch. That is not an omission — the lease wants the
//! shared copy *as you last saw it*, and the lease is fufu's own record of
//! that, `refs/fufu/seen/<branch>` ([`crate::seen`]), not the tracking
//! ref. No fetch moves the record, fufu's or anyone's: an editor's
//! background fetch, `ff git fetch`, and `ff pull --dry-run` all move the
//! tracking ref and leave the record where the last report put it. A
//! tracking ref standing off the record is a copy that moved since you
//! looked, and [`plan`] refuses it here, before the wire, the way git would
//! have refused it at the wire. Going to the network first would ask git to
//! protect you against a change you just accepted sight unseen. `ff pull`
//! is how you look.
//!
//! What it decides is small: is there anywhere to send this, is the exit
//! blocked, does the remote already have it, and if not, what the push does
//! to the shared copy. The network itself is somebody else's job — this
//! module hands back a plan and never spawns anything.
//!
//! Which branches a run visits is the caller's to say, through [`Scope`]
//! and [`choose`]: the branch underfoot, or the ones named. Each is planned
//! on its own by [`plan`], since each has its own shared copy and goes out
//! under its own lease; nothing beneath or above a branch comes with it,
//! because a push reads no base and moves no local ref.
//!
//! And then, once somebody else has made the call, it records it. That is
//! the whole of what push writes, and [`record`] is a second entry point
//! rather than a step inside [`push`] because the send happens between
//! them: what it writes is a fact, written after the wire agreed, and there
//! is no local ref to diff a write-ahead claim against. If either write is
//! lost after a successful push, the next pull reads the remote as theirs
//! and replays, which never loses work.
//!
//! Three marks, not one, and [`crate::published`] is where the reason
//! lives: the note is the record a person reads and is rewound by `ff undo`
//! with everything else above the landing; the published pointer is the
//! answer pull needs and the seen pointer is the next push's lease, and
//! neither is a thing undo may step back, because undo cannot step back
//! the wire.

use std::collections::HashSet;

use crate::model::{Push, PushReport, PushShape};
use crate::ops::record::{OpRecord, Published, observe_refs};
use crate::ops::{OpId, OpKind};
use crate::preflight::Preflight;
use crate::{Error, Provenance, Result};

pub struct PushOptions {
    /// Decide the plan, write nothing, send nothing. The one thing worth
    /// previewing here is which push this would be — creating a shared copy,
    /// replacing one, putting back one that was deleted, and rolling one
    /// back are four different acts wearing one verb.
    pub dry_run: bool,
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// Which branches a run visits: the branch underfoot, or the ones named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Current,
    Named(Vec<String>),
}

/// What a scope resolves to: whether the branch underfoot is in the run,
/// and every branch in it, in the order the ref namespace lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub current: bool,
    pub branches: Vec<String>,
}

/// The branches a scope reaches. `Current` is the branch underfoot alone.
/// `Named` is exactly the branches asked for: a name resolves the way `ff
/// restack` resolves one, an unambiguous prefix included, and one that
/// resolves to nothing is refused before any network. A branch named twice
/// is in the run once, and the run is ordered the way the ref namespace
/// lists it, whatever order the names came in. Nothing comes along with a
/// named branch: not the bases beneath it, which `ff pull` brings in
/// because a branch lines up with its base through them, and not what is
/// stacked above, since a push reads no base and moves no local ref.
pub fn choose(repo: &gix::Repository, current: &str, scope: &Scope) -> Result<Chosen> {
    let raw = match scope {
        Scope::Current => {
            return Ok(Chosen {
                current: true,
                branches: vec![current.to_string()],
            });
        }
        Scope::Named(raw) => raw,
    };
    let chosen: HashSet<String> = raw
        .iter()
        .map(|name| crate::switch::resolve_branch(repo, name))
        .collect::<Result<_>>()?;
    Ok(Chosen {
        current: chosen.contains(current),
        branches: crate::switch::branch_names(repo)?
            .into_iter()
            .filter(|n| chosen.contains(n))
            .collect(),
    })
}

/// Capture first, like every verb. Push changes nothing locally, so it
/// records no operation of its own — but the tree is snapshotted and
/// foreign motion is reconciled before anything leaves, which is the point
/// of the floor. One capture opens a run however many branches it sends. A
/// dry run reads and writes nothing, so it takes nothing: same rule as `ff
/// trim -n`.
pub fn begin(
    repo: &gix::Repository,
    dry_run: bool,
    now: Option<i64>,
    prov: &Provenance,
) -> Result<Option<crate::ops::verb::VerbContext>> {
    if dry_run {
        return Ok(None);
    }
    Ok(Some(crate::ops::verb::begin_verb(repo, prov, now)?))
}

/// The plan for the branch underfoot, after the run's capture: [`begin`]
/// then [`plan`], for the caller that sends one branch.
pub fn push(
    repo: &gix::Repository,
    pre: &Preflight,
    opts: PushOptions,
    prov: &Provenance,
) -> Result<(PushReport, Option<crate::ops::verb::VerbContext>)> {
    let ctx = begin(repo, opts.dry_run, opts.now, prov)?;
    let push = plan(repo, pre)?;
    Ok((
        PushReport {
            branch: pre.branch.clone(),
            push,
            dry_run: opts.dry_run,
        },
        ctx,
    ))
}

/// What the push of one branch is, decided from refs alone: nowhere to send
/// it, the exit blocked, the shared copy already level, the copy moved
/// since you last looked, or the push and the lease it goes out under.
///
/// The lease is the seen record, and the tracking tip is checked against
/// it here rather than left to the wire: a tracking ref that stands off the
/// record was moved by a fetch behind fufu's back, and a lease read from it
/// would vouch for a tip nobody looked at. A fast-forward of the tracking
/// tip goes whatever the record says, since no lease value can take
/// anything off the copy; that is also how a branch with no record at all
/// — one from before the record existed, or whose upstream git set — gets
/// one, written by [`record`] afterwards.
pub fn plan(repo: &gix::Repository, pre: &Preflight) -> Result<Push> {
    let push = if crate::held::of(repo, &pre.branch)?.is_some() {
        // The exits-blocked discipline: a held rewrite means the branch's
        // commits are not what they will be, and sending them would put out
        // a state fufu is about to rewrite out from under.
        Push::Blocked
    } else {
        match pre.remote.as_ref() {
            None => Push::NoRemote,
            Some(remote) => {
                let tip = crate::preflight::branch_tip(repo, &pre.branch)?;
                match pre.tracking.as_ref() {
                    // No upstream at all: the push is what creates one and
                    // starts tracking it.
                    None => Push::Create {
                        remote: remote.clone(),
                        remote_branch: pre.branch.clone(),
                        tip: tip.to_string(),
                    },
                    Some(tracking) if tracking.tip == Some(tip) => Push::UpToDate,
                    Some(tracking) => {
                        if let Some(why) = refuse(repo, tracking, tip)? {
                            return Ok(Push::Refused {
                                remote: remote.clone(),
                                remote_branch: tracking.remote_branch.clone(),
                                tip: tip.to_string(),
                                why,
                            });
                        }
                        // Configured, and either standing somewhere else or
                        // not there at all. Every one of these is the same
                        // push under a different lease: the tip as last
                        // seen, or the empty string, which is git's
                        // spelling for *must not exist*. Typing `ff push`
                        // is saying that out loud; when pushing was a
                        // default, this case needed a flag to mean it. Only
                        // the sentence afterwards differs, and `shape` is
                        // what tells the four apart.
                        Push::Push {
                            remote: remote.clone(),
                            remote_branch: tracking.remote_branch.clone(),
                            lease: tracking.tip.map(|id| id.to_string()).unwrap_or_default(),
                            tip: tip.to_string(),
                            shape: shape(repo, pre, tracking, tip)?,
                        }
                    }
                }
            }
        }
    };
    Ok(push)
}

/// Whether the tracking tip is one the lease can vouch for, and why not
/// when it is not. An absent tracking ref is never refused: the empty
/// lease says *must not exist*, and `shape` decides whether that is a
/// first copy or a re-creation. A fast-forward of the tracking tip is
/// never refused either, whatever the record says: it sends commits and
/// takes none off, so there is nothing a lease could be vouching for, and
/// the wire still catches a move that lands after this reading.
fn refuse(
    repo: &gix::Repository,
    tracking: &crate::preflight::Tracking,
    tip: gix::ObjectId,
) -> Result<Option<crate::model::Refusal>> {
    let Some(now) = tracking.tip else {
        return Ok(None);
    };
    if tracking.seen == Some(now) {
        return Ok(None);
    }
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(tip, &[now])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.contains(&now) {
        return Ok(None);
    }
    let behind = crate::upstream::count_exclusive(repo, now, &bases)?;
    Ok(Some(match tracking.seen {
        Some(seen) => crate::model::Refusal::Moved {
            seen: seen.to_string(),
            now: now.to_string(),
            behind,
        },
        None => crate::model::Refusal::Unseen {
            now: now.to_string(),
            behind,
        },
    }))
}

/// The coded error a refused row carries: what the copy holds that the
/// branch does not, and the way through, which is a pull and then the push
/// again. `None` for every other push.
pub fn refusal(branch: &str, push: &Push) -> Option<Error> {
    let Push::Refused {
        remote,
        remote_branch,
        why,
        ..
    } = push
    else {
        return None;
    };
    let exits = vec![format!("ff pull {branch}"), format!("ff push {branch}")];
    Some(match why {
        // The same state the wire's refusal names, so the same id: nothing
        // sent, and the commits are still here.
        crate::model::Refusal::Moved { behind, .. } => Error::coded(
            "push/lease-refused",
            format!(
                "{remote}/{remote_branch} moved since you last looked ({behind} commit(s) you \
                 have not taken in), so nothing was pushed — ff pull takes them in"
            ),
            exits,
        ),
        crate::model::Refusal::Unseen { behind, .. } => Error::coded(
            "push/unseen",
            format!(
                "fufu has no record of where you last looked at {remote}/{remote_branch}, and \
                 it holds {behind} commit(s) {branch} does not — nothing was pushed"
            ),
            exits,
        ),
    })
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
    let Some(tracking_tip) = tracking.tip else {
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
        .merge_bases_many(tip, &[tracking_tip])
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
/// The one local trace a push leaves is on a branch whose upstream wore
/// another branch's name: the create that gives it a copy of its own also
/// rewrites `branch.<n>.merge`, so the base that upstream named is recorded
/// as the parent here, before the row goes down, and the transition rides
/// the row so a move of the log past it takes the record back with it. A
/// parent already recorded stands.
///
/// The pointer goes down after the note, and it is the half the readers use.
/// See [`crate::published`] for why the row alone would not do.
pub fn record(
    repo: &gix::Repository,
    pre: &Preflight,
    report: &PushReport,
    ctx: &crate::ops::verb::VerbContext,
    prov: &Provenance,
) -> Result<Option<OpId>> {
    let (remote, remote_branch, from, to) = match &report.push {
        Push::Create {
            remote,
            remote_branch,
            tip,
        } => (remote, remote_branch, None, tip),
        Push::Push {
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
        Push::NotNamed | Push::NoRemote | Push::Blocked | Push::UpToDate | Push::Refused { .. } => {
            return Ok(None);
        }
    };

    let mut record = OpRecord::new(
        "push",
        format!("pushed {} to {remote}/{remote_branch}", pre.branch),
        ctx.now,
    );
    record.published = Some(Published {
        remote: remote.clone(),
        remote_branch: remote_branch.clone(),
        from,
        to: to.clone(),
    });
    if let (Push::Create { .. }, Some(parent)) = (&report.push, pre.upstream_alias.as_ref()) {
        let mut meta = crate::branchmeta::read(repo, &pre.branch)?;
        if meta.parent.is_none() {
            meta.parent = Some(parent.clone());
            crate::branchmeta::write(repo, &pre.branch, &meta)?;
            record.parent = Some(crate::ops::record::ParentTransition {
                branch: pre.branch.clone(),
                old: None,
                new: Some(parent.clone()),
            });
        }
    }
    // The one pin. `to` is the whole answer this row exists to give, and a
    // row naming a sha gc has since collected would be a dangling claim.
    let pins: Vec<gix::ObjectId> = gix::ObjectId::from_hex(to.as_bytes()).into_iter().collect();
    let head = crate::head::head_state(repo)?;
    let id = crate::ops::verb::append_op(
        repo,
        OpKind::Note,
        crate::ops::verb::VerbOp {
            record,
            // Push moves no local ref, so the planned state is the state:
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
        // The copy stands where this push left it, and the person watched
        // it go there: the next lease is this tip.
        crate::seen::mark(repo, &pre.branch, oid, ctx.now)?;
    }
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use ff_testsupport::Fixture;

    use super::*;
    use crate::model::Refusal;
    use crate::preflight::{Verb, preflight_branch};

    /// `main` two commits deep, `other` one commit off the root, and `main`
    /// tracking `origin/main` with the tracking ref written by hand: the
    /// remote is never reached, since `plan` reads refs alone.
    fn tracked() -> (Fixture, String, String, String) {
        let fx = Fixture::new();
        fx.write("root.txt", "root\n");
        let root = fx.commit("root");
        fx.write("a.txt", "a\n");
        let a = fx.commit("a");
        fx.git(&["switch", "-q", "-c", "other", &root]);
        fx.write("theirs.txt", "theirs\n");
        let theirs = fx.commit("theirs");
        fx.git(&["switch", "-q", "main"]);
        fx.set_config("remote.origin.url", "/nonexistent/remote.git");
        fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
        fx.set_config("branch.main.remote", "origin");
        fx.set_config("branch.main.merge", "refs/heads/main");
        (fx, root, a, theirs)
    }

    fn plan_main(fx: &Fixture, tracking: Option<&str>, seen: Option<&str>) -> Push {
        match tracking {
            Some(sha) => fx.git(&["update-ref", "refs/remotes/origin/main", sha]),
            None => fx.git(&["update-ref", "-d", "refs/remotes/origin/main"]),
        };
        match seen {
            Some(sha) => fx.git(&["update-ref", "refs/fufu/seen/main", sha]),
            None => fx.git(&["update-ref", "-d", "refs/fufu/seen/main"]),
        };
        let repo = fx.repo();
        let pre = preflight_branch(&repo, Verb::Push, "main", None).expect("preflight");
        plan(&repo, &pre).expect("plan")
    }

    /// The lease is the seen record, and the plan reads the tracking tip
    /// against it: level with the record, the push goes under it; off the
    /// record, refused as moved, with the count of what the copy holds;
    /// no record, refused as unseen. A fast-forward of the tracking tip
    /// goes either way, and no tracking ref keeps the empty lease as
    /// before, whatever the record says.
    #[test]
    fn the_plan_reads_the_tracking_tip_against_the_seen_record() {
        let (fx, root, a, theirs) = tracked();

        match plan_main(&fx, Some(&root), Some(&root)) {
            Push::Push { lease, tip, .. } => {
                assert_eq!(lease, root);
                assert_eq!(tip, a);
            }
            other => panic!("level with the record: {other:?}"),
        }

        match plan_main(&fx, Some(&theirs), Some(&root)) {
            Push::Refused {
                why: Refusal::Moved { seen, now, behind },
                ..
            } => {
                assert_eq!(seen, root);
                assert_eq!(now, theirs);
                assert_eq!(behind, 1);
            }
            other => panic!("off the record: {other:?}"),
        }

        for seen in [None, Some(theirs.as_str())] {
            match plan_main(&fx, Some(&root), seen) {
                Push::Push { lease, .. } => assert_eq!(lease, root),
                other => panic!("a fast-forward under {seen:?}: {other:?}"),
            }
        }

        match plan_main(&fx, Some(&theirs), None) {
            Push::Refused {
                why: Refusal::Unseen { now, behind },
                ..
            } => {
                assert_eq!(now, theirs);
                assert_eq!(behind, 1);
            }
            other => panic!("no record, diverged: {other:?}"),
        }

        for seen in [Some(root.as_str()), None] {
            match plan_main(&fx, None, seen) {
                Push::Push { lease, .. } => assert_eq!(lease, ""),
                other => panic!("no tracking ref: {other:?}"),
            }
        }
    }

    /// A refused row's error: the same id as the wire's refusal for a copy
    /// that moved, a new one for a copy never looked at, and both exits
    /// name the branch.
    #[test]
    fn a_refused_row_carries_the_coded_error() {
        let (fx, root, _, theirs) = tracked();
        let moved = plan_main(&fx, Some(&theirs), Some(&root));
        let err = refusal("main", &moved).expect("moved is refused");
        assert_eq!(err.id(), "push/lease-refused");
        assert!(err.to_string().contains("1 commit(s)"), "{err}");
        assert_eq!(err.exits(), ["ff pull main", "ff push main"]);

        let unseen = plan_main(&fx, Some(&theirs), None);
        let err = refusal("main", &unseen).expect("unseen is refused");
        assert_eq!(err.id(), "push/unseen");
        assert!(err.to_string().contains("no record"), "{err}");

        assert!(refusal("main", &plan_main(&fx, Some(&root), None)).is_none());
    }
}
