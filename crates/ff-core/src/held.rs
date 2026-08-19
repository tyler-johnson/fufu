//! A rewrite that conflicted and is waiting, and the resolution session that
//! works on it. What is recorded is the verb's own question — the branch, the
//! target, what it was asked to become — and never the plan it could not
//! finish computing. `ff resolve` re-derives the plan from the repository as
//! it stands, which is cache-not-authority taken literally: nothing has to be
//! pinned, because every input is either a ref or the working tree; "the
//! pending rewrite replays over whatever you add" costs nothing, because the
//! replan sees what you added; and a target that has since gone, or moved out
//! of history, expires the hold loudly at the moment somebody asks rather
//! than replaying something stale. Both records ride on the branch's own
//! metadata, exactly as an editing session does.

use gix::prelude::ObjectIdExt;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::futures::At;
use crate::ops::record::{HeldTransition, ResolveTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, verb};
use crate::snapshot::Provenance;

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}

/// What a held rewrite was asked to do, in terms that can be asked again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "kebab-case")]
pub enum Intent {
    /// `ff restack`: replay `branch` onto whatever `onto` resolves to now.
    /// `onto` is a ref — `refs/heads/main`, or `refs/remotes/origin/feature` —
    /// resolved fresh at replan time, because the base moving is the ordinary
    /// case and re-reading it is the point. A bare short name recorded before
    /// full refs were written still resolves.
    Restack { branch: String, onto: String },
    /// `ff done`: land the editing session on `session`.
    Done { session: String },
    /// `ff absorb`: fold the open change into `into`. The tree is not
    /// recorded — the working tree still holds it, and re-deriving from the
    /// working tree as it stands now is what makes the hold durable.
    Absorb { into: String, paths: Vec<String> },
    /// `ff lift`: take `paths` out of `from` and back into the open change.
    Lift { from: String, paths: Vec<String> },
}

/// A rewrite that conflicted and is waiting, recorded as one field of the
/// branch's metadata exactly as an editing session is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub intent: Intent,
    /// Where it stopped when it was recorded — a commit the replay could not
    /// reapply, or the open change, which cannot come along. Disclosure only:
    /// `ff resolve` recomputes both this and `paths` before it materializes
    /// anything, so what is stored here can only ever be what the report said
    /// at the time. The type is `futures::At` because a conflict lands in one
    /// of exactly two places and the probe already names them.
    pub at: crate::futures::At,
    pub paths: Vec<String>,
    /// When it was held, seconds since the epoch. Not the operation's id: the
    /// record carrying the hold is built before the append returns one, so an
    /// id here would either be circular or empty after an undo restored it
    /// from that record. The question people ask is how long it has stood.
    pub time: i64,
}

/// What a hold has to become before anything can simulate or land it: the
/// three arguments `rewrite::plan` and `rewrite::chain` both take. Derived
/// from the repository as it stands, never stored — which is the whole point
/// of recording the question instead of the answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Replan {
    pub target: gix::ObjectId,
    pub tip: gix::ObjectId,
    pub change: crate::rewrite::Change,
}

/// An open resolution session. Not a branch — you stay where you stand and
/// the working tree carries the markers — so unlike an editing session this
/// is one field of your *own* branch's metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolve {
    /// The hold being resolved. Copied rather than referenced, so a hold
    /// rewritten underneath an open session cannot change what the session
    /// is resolving halfway through.
    pub hold: Held,
    /// The marker tree this session materialized, full sha. `ff done` re-runs
    /// the chain and refuses if it does not arrive here again — the same
    /// self-invalidating key the futures cache is built on.
    pub from: String,
    /// Each step's subject, oldest-first, so a report can name what landed
    /// without replaying to find out.
    pub steps: Vec<String>,
    /// The working tree, as a tree, at the moment the session opened.
    ///
    /// The one input a session has to keep. Everything else a hold needs is a
    /// ref, and `replan` asks for it again; the open change is not, and
    /// `ff resolve` spends it — `done`, `absorb` and `lift` fold it into the
    /// chain's trees and then the markers take the working tree's place, so
    /// by `ff done` there is nothing left there to re-read. Recording it is
    /// what lets the replan arrive at the same plan twice, which is what
    /// makes `held/moved` mean "the world moved" rather than "you resolved".
    /// A restack reads no working tree and ignores this.
    #[serde(default)]
    pub open: Option<String>,
}

/// The hold standing on `branch`, if one does.
pub fn of(repo: &gix::Repository, branch: &str) -> Result<Option<Held>> {
    Ok(crate::branchmeta::read(repo, branch)?.held)
}

/// One hold per branch. Composition — several holds on one stack, and what
/// queues behind what — is a question fufu has not answered, so a second
/// *conflicting* rewrite refuses rather than guessing an order. A rewrite
/// that would succeed is not competing for anything and is allowed through,
/// which is why every caller asks this where the hold would be recorded and
/// not at the top of the verb.
///
/// `verb_past` completes "nothing was ___": `restacked`, `absorbed`,
/// `lifted`, `landed`.
pub(crate) fn refuse_if_held(repo: &gix::Repository, branch: &str, verb_past: &str) -> Result<()> {
    if let Some(existing) = of(repo, branch)? {
        let where_held = match &existing.at {
            At::Commit { id, subject } => format!(
                "{} \"{}\"",
                short(
                    repo,
                    gix::ObjectId::from_hex(id.as_bytes()).expect("the probe's ids are shas")
                ),
                subject
            ),
            At::OpenChange => "your open change".to_string(),
        };
        return Err(Error::coded(
            "held/already-held",
            format!("{branch} already has a rewrite held at {where_held}: nothing was {verb_past}"),
            vec![
                "ff resolve".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    Ok(())
}

/// Record a hold on `branch`, or clear it with `None`.
pub fn set(repo: &gix::Repository, branch: &str, held: Option<Held>) -> Result<()> {
    let mut meta = crate::branchmeta::read(repo, branch)?;
    meta.held = held;
    crate::branchmeta::write(repo, branch, &meta)
}

/// Turn a hold back into a plan against the repository as it stands.
///
/// This is where cache-not-authority becomes concrete. A hold is never
/// replayed from what it stored; it is asked again. If the world has moved so
/// far that the question no longer has an answer — the base branch is gone,
/// the target is no longer in history, the session was ended by hand — the
/// hold has outlived its meaning and this says so loudly rather than
/// resolving something nobody asked for.
pub fn replan(repo: &gix::Repository, held: &Held) -> Result<Replan> {
    replan_at(repo, held, None)
}

/// `replan`, with the open change handed in rather than read off the working
/// tree. `ff done` finishing a resolution passes the tree the session
/// recorded, because the markers are standing in the working tree by then;
/// every other caller passes `None` and the working tree answers for itself.
pub fn replan_at(
    repo: &gix::Repository,
    held: &Held,
    open: Option<gix::ObjectId>,
) -> Result<Replan> {
    // A failure here is reported as an expiration, but the hold itself is
    // left standing: expiring it is a decision with an operation attached, and
    // it belongs to the verb that acts on the hold, not to a function that
    // only answers a question.
    match &held.intent {
        Intent::Restack { branch, onto } => {
            crate::restack::replan_restack(repo, branch, onto).map_err(|e| expired("restack", e))
        }
        Intent::Done { session } => {
            crate::done::replan_done(repo, session, open).map_err(|e| expired("done", e))
        }
        Intent::Absorb { into, paths } => {
            let into_id = gix::ObjectId::from_hex(into.as_bytes())
                .map_err(|e| expired("absorb", Error::msg(e.to_string())))?;
            crate::absorb::replan_absorb(repo, Some(into_id), paths, open)
                .map_err(|e| expired("absorb", e))
        }
        Intent::Lift { from, paths } => {
            let from_id = gix::ObjectId::from_hex(from.as_bytes())
                .map_err(|e| expired("lift", Error::msg(e.to_string())))?;
            crate::absorb::replan_lift(repo, Some(from_id), paths).map_err(|e| expired("lift", e))
        }
    }
}

/// Wrap a replan failure as an expiration: the hold named a thing that is no
/// longer where it stood when the hold was recorded, so the hold is stale
/// rather than the command wrong. `{why}` keeps the underlying message.
fn expired(verb: &str, why: Error) -> Error {
    Error::coded(
        "held/expired",
        format!("the held {verb} cannot be replanned: {why}; the hold is stale"),
        vec!["ff resolve --abandon".into(), "ff status".into()],
    )
}

/// What a hold needs from the verb recording it. Bundled because the four
/// always travel together and the alternative is an eight-argument function.
pub(crate) struct Recording<'a> {
    pub ctx: &'a crate::ops::VerbContext,
    pub prov: &'a Provenance,
    pub argv: Vec<String>,
    pub now: i64,
}

/// Record a hold as an operation. Nothing moves — the planned end state is
/// the present on every axis but the branch's metadata — which is the same
/// slim shape `ff describe` writes for a pending description. The operation
/// is what makes a hold undoable and what `ff trim` eventually ages out.
pub(crate) fn record(
    repo: &gix::Repository,
    rec: Recording<'_>,
    branch: &str,
    held: &Held,
    summary: String,
) -> Result<()> {
    let Recording {
        ctx,
        prov,
        argv,
        now,
    } = rec;
    let old = of(repo, branch)?;
    let head = crate::head::head_state(repo)?;

    let table = observe_refs(repo)?;
    let mut record = OpRecord::new("hold", summary, now);
    record.argv = argv;
    record.held = Some(HeldTransition {
        branch: branch.to_string(),
        old,
        new: Some(held.clone()),
    });
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned: table,
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: branch.to_string(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &[],
        },
        now,
    )?;
    set(repo, branch, Some(held.clone()))
}

/// The resolution session open on `branch`, if one is.
pub fn resolving(repo: &gix::Repository, branch: &str) -> Result<Option<Resolve>> {
    Ok(crate::branchmeta::read(repo, branch)?.resolving)
}

/// Open or close a resolution session on `branch`.
pub fn set_resolving(repo: &gix::Repository, branch: &str, resolve: Option<Resolve>) -> Result<()> {
    let mut meta = crate::branchmeta::read(repo, branch)?;
    meta.resolving = resolve;
    crate::branchmeta::write(repo, branch, &meta)
}

/// The two transitions a clearing landing records on its op: each carries what
/// stood on the branch as `old` and `None` as `new`, so one `ff undo` of the
/// landing puts the hold and the session back together. `None` on an axis the
/// clearing carried nothing to clear, so an ordinary landing records neither.
pub(crate) fn clearing_transitions(
    clearing: &crate::rewrite::Clearing,
) -> (Option<HeldTransition>, Option<ResolveTransition>) {
    let held = clearing.held.as_ref().map(|old| HeldTransition {
        branch: clearing.branch.clone(),
        old: Some(old.clone()),
        new: None,
    });
    let resolving = clearing.resolve.as_ref().map(|old| ResolveTransition {
        branch: clearing.branch.clone(),
        old: Some(old.clone()),
        new: None,
    });
    (held, resolving)
}
