//! A rewrite that conflicted and is waiting, and the resolution session that
//! works on it. What is recorded is the verb's own question — the branch, the
//! target, what it was asked to become — and never the plan it could not
//! finish computing. `ff resolve` re-derives the plan from the repository as
//! it stands, which is cache-not-authority taken literally: nothing has to be
//! pinned, because every input is either a ref or the working tree; "the
//! pending rewrite replays over whatever you add" costs nothing, because the
//! replan sees what you added; and a target that has since gone, or moved out
//! of history, expires the hold loudly at the moment somebody asks rather
//! than replaying something stale. The hold rides on the branch's own
//! metadata, and so does the record of the resolution session open on it,
//! which names the session branch the way `ff edit` names its own.

use serde::{Deserialize, Serialize};

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::futures::At;
use crate::model::DroppedHold;
use crate::ops::record::{
    HeldTransition, RefsTable, ResolveTransition, SessionTransition, observe_refs,
};
use crate::ops::{OpKind, OpRecord, RefTransition, verb};
use crate::park::ArrivePlan;
use crate::refs;
use crate::snapshot::Provenance;
use crate::stash;

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
    /// `ff resolve`'s merge door, and `ff merge` once it exists: take `onto`
    /// into `branch` by one commit with two parents. `onto` is a full ref
    /// resolved fresh at replan time, as a restack's is; the base moving is
    /// the ordinary case. `message` is the commit's message when a person
    /// gave one.
    Merge {
        branch: String,
        onto: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// `ff done`: land the editing session on `session`.
    Done { session: String },
    /// `ff absorb`: the move, in the spelling it was typed. The two move
    /// variants carry one body — the sources, the target, the target's
    /// message, the path filter — and differ only in the tag, because `ff
    /// status` and the hold's re-run spell the verb the way it was typed.
    /// Each end is a full sha or `@`, the open change. The open tree is not
    /// recorded — the working tree still holds it, and re-deriving from the
    /// working tree as it stands now is what makes the hold durable. A hold
    /// recorded before both verbs took both ends reads back with the verb's
    /// old defaults filled in.
    Absorb {
        #[serde(default = "open_only")]
        from: Vec<String>,
        into: String,
        #[serde(default)]
        message: Option<String>,
        paths: Vec<String>,
    },
    /// `ff lift`: the same move, spelled the other way.
    Lift {
        #[serde(deserialize_with = "string_or_seq")]
        from: Vec<String>,
        #[serde(default = "open")]
        into: String,
        #[serde(default)]
        message: Option<String>,
        paths: Vec<String>,
    },
    /// `ff switch`: the change parked on `branch` — its open commit, `open`
    /// — conflicts with the tip it arrived on. The replay is that one commit
    /// onto the branch's tip, and `ff resolve` lays it into the open change.
    Arrive { branch: String, open: String },
}

/// The move a held absorb or lift asks for, read off either variant.
pub(crate) struct MoveIntent<'a> {
    pub verb: crate::absorb::MoveVerb,
    pub from: &'a [String],
    pub into: &'a str,
    pub message: Option<&'a str>,
    pub paths: &'a [String],
}

impl Intent {
    /// The move a held absorb or lift asks for: the verb as spelled, the
    /// sources, the target, the message, and the path filter. `None` for
    /// every other intent.
    pub(crate) fn as_move(&self) -> Option<MoveIntent<'_>> {
        match self {
            Intent::Absorb {
                from,
                into,
                message,
                paths,
            } => Some(MoveIntent {
                verb: crate::absorb::MoveVerb::Absorb,
                from,
                into,
                message: message.as_deref(),
                paths,
            }),
            Intent::Lift {
                from,
                into,
                message,
                paths,
            } => Some(MoveIntent {
                verb: crate::absorb::MoveVerb::Lift,
                from,
                into,
                message: message.as_deref(),
                paths,
            }),
            _ => None,
        }
    }
}

/// Absorb's default sources, for a hold recorded before `--from` existed.
fn open_only() -> Vec<String> {
    vec!["@".to_string()]
}

/// Lift's default target, for a hold recorded before `--into` existed.
fn open() -> String {
    "@".to_string()
}

/// A lift's `from`: one sha on a record written before a lift could name a
/// run, a list since.
fn string_or_seq<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(one) => vec![one],
        OneOrMany::Many(many) => many,
    })
}

/// What a rewrite verb does when the replay on the branch underfoot
/// conflicts: record the hold and stop, exit 3, or record it and open the
/// resolution session in the same run, the way `ff resolve`'s merge door
/// does. `fufu.onConflict` chooses the standing answer and `--resolve` or
/// `--no-resolve` on the verb overrides it for one run. Only the branch
/// HEAD stands on can open: a hold on a named other branch, a cascade
/// child, or a non-current branch in a pull holds and is named either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnConflict {
    /// Record the hold and stop.
    Hold,
    /// Record the hold and open the session over it.
    Resolve,
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

/// An open resolution session, recorded on the branch the hold stands on.
/// The session itself is a branch: `ff resolve` mints an anonymous branch
/// at a commit carrying the marker tree and switches to it, exactly as
/// `ff edit` does, and that branch's own metadata carries an ordinary
/// [`branchmeta::Session`] pointing back here. This record is what lets
/// the branch you left refuse a second `ff resolve` with the session's
/// name, and what `ff done` on the session reads to find the hold.
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
    /// The session branch: the anonymous branch carrying the markers, with
    /// HEAD on it while the session is open. Empty on a record written
    /// before sessions were branches (v0.5.0 through v0.12.1), whose markers
    /// stood in the working copy of the branch itself; such a record reads
    /// back as expired, and `ff resolve --abandon` clears it.
    #[serde(default)]
    pub session: String,
    /// The fixes earlier rounds attributed, kept as the steps' resolutions.
    /// A fix that uncovers the next conflict rolls the session to another
    /// round rather than landing, and the round's marker tree is the chain
    /// run with these folded in, so `from` is that tree and every round
    /// re-derives with them. Empty on a session in its first round.
    #[serde(default)]
    pub resolutions: Vec<crate::rewrite::Resolution>,
    /// The files carrying markers in this round's tree, sorted and deduped:
    /// the count `ff status` shows. Empty on a record written before rounds,
    /// which reads as the hold's own paths.
    #[serde(default)]
    pub files: Vec<String>,
}

/// The hold standing on `branch`, if one does.
pub fn of(repo: &gix::Repository, branch: &str) -> Result<Option<Held>> {
    Ok(crate::branchmeta::read(repo, branch)?.held)
}

/// The verb that held, in the spelling the reports and the re-run carry:
/// the same names `Intent` serializes to.
pub fn verb_of(held: &Held) -> String {
    match &held.intent {
        Intent::Restack { .. } => "restack",
        Intent::Merge { .. } => "merge",
        Intent::Done { .. } => "done",
        Intent::Absorb { .. } => "absorb",
        Intent::Lift { .. } => "lift",
        Intent::Arrive { .. } => "switch",
    }
    .to_string()
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
        return Err(already_held(branch, &existing, verb_past));
    }
    Ok(())
}

/// The one-hold-per-branch refusal, for a hold read already.
fn already_held(branch: &str, existing: &Held, verb_past: &str) -> Error {
    let where_held = match &existing.at {
        At::Commit { id, subject } => format!(
            "{} \"{}\"",
            crate::sha::short_oid(
                gix::ObjectId::from_hex(id.as_bytes()).expect("the probe's ids are shas")
            ),
            subject
        ),
        At::OpenChange => "your open change".to_string(),
    };
    Error::coded(
        "held/already-held",
        format!("{branch} already has a rewrite held at {where_held}: nothing was {verb_past}"),
        vec![
            "ff resolve".into(),
            "ff resolve --abandon".into(),
            "ff status".into(),
        ],
    )
}

/// A branch whose hold has a resolution session open is being worked on
/// elsewhere: the way to it is a switch, not this verb.
pub(crate) fn refuse_if_resolving(repo: &gix::Repository, branch: &str) -> Result<()> {
    if let Some(open) = resolving(repo, branch)? {
        if open.session.is_empty() {
            return Err(predates_sessions(branch));
        }
        return Err(Error::coded(
            "held/resolving",
            format!("a resolution of {branch} is open on {}", open.session),
            vec![
                format!("ff switch {}", open.session),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    Ok(())
}

/// What a rewrite does to the branch's base.
#[derive(Clone, Copy)]
pub(crate) enum Effect<'a> {
    /// `ff restack --onto` and `ff fold`: the branch's base becomes `onto`,
    /// a full ref.
    Reaim { onto: &'a str },
    /// absorb, lift, describe, done's landing, a bare restack: the base
    /// stays.
    Keep,
}

/// What a rewrite does with the hold standing on the branch it rewrites.
/// What a hold carries decides it: a `Restack` or a `Merge` carries no
/// content — its `onto` is a ref resolved fresh, and `at` and `paths` are
/// disclosure — so a rewrite can drop it, keep it, or land it; an `Absorb`,
/// `Lift`, `Done`, or `Arrive` carries work that is not on the branch yet,
/// and a rewrite under it would lose that work, so it refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Disposition {
    /// No hold stands.
    None,
    /// The hold's question is moot: recorded `old → None`, and the report
    /// names it.
    Drop(Held),
    /// The hold stays: `at` follows the rewrite map; `paths` is resolve's
    /// to recompute.
    Remap(Held),
    /// The rewrite is the held restack landing: recorded `old → None`, no
    /// line.
    Clear(Held),
}

/// The hold on `branch`, sorted by what a rewrite with `effect` does to it.
/// Refuses any hold whose resolution session is open, HEAD in it or parked,
/// with `held/resolving`, and a content-carrying hold — `Absorb`, `Lift`,
/// `Done`, `Arrive` — with `held/already-held` in `refuse_if_held`'s words
/// (`verb_past` completes "nothing was ___"). A `Restack { onto }` or
/// `Merge { onto }` hold: `Reaim` at a different `onto` is `Drop`; `Reaim`
/// at the hold's own `onto` is `Clear`; `Keep` is `Remap`. `onto` is
/// compared by full ref: the hold's is `base.full` by construction, and
/// callers pass `Onto.full`.
pub(crate) fn before_rewrite(
    repo: &gix::Repository,
    branch: &str,
    effect: Effect<'_>,
    verb_past: &str,
) -> Result<Disposition> {
    refuse_if_resolving(repo, branch)?;
    let Some(held) = of(repo, branch)? else {
        return Ok(Disposition::None);
    };
    match &held.intent {
        Intent::Restack { onto, .. } | Intent::Merge { onto, .. } => Ok(match effect {
            Effect::Reaim { onto: new } if new == onto => Disposition::Clear(held),
            Effect::Reaim { .. } => Disposition::Drop(held),
            Effect::Keep => Disposition::Remap(held),
        }),
        Intent::Absorb { .. }
        | Intent::Lift { .. }
        | Intent::Done { .. }
        | Intent::Arrive { .. } => Err(already_held(branch, &held, verb_past)),
    }
}

impl Disposition {
    /// Restack alone: a bare `ff restack` whose base is the hold's own
    /// `onto` lands the hold, so a `Remap` whose hold names `base_full`
    /// becomes `Clear`. A held merge of that base clears too: the replay
    /// flattens the in-range merge and lands the branch on the base, so
    /// the merge's question is moot.
    pub(crate) fn landing_on(self, base_full: &str) -> Self {
        match self {
            Disposition::Remap(held)
                if matches!(
                    &held.intent,
                    Intent::Restack { onto, .. } | Intent::Merge { onto, .. } if onto == base_full
                ) =>
            {
                Disposition::Clear(held)
            }
            other => other,
        }
    }

    /// The record's transition. `Drop`/`Clear`: `old: Some, new: None`.
    /// `Remap`: `new` is the hold with `At::Commit { id, .. }` replaced by
    /// the `Rewrite` whose `old == id`, if any; an id in no rewrite
    /// (dropped, or below the range) and `At::OpenChange` stay as recorded,
    /// since resolve recomputes `at` anyway. `paths` and `time` untouched.
    /// `None` for `None`.
    pub(crate) fn transition(
        &self,
        branch: &str,
        rewrites: &[crate::rewrite::Rewrite],
    ) -> Option<HeldTransition> {
        match self {
            Disposition::None => None,
            Disposition::Drop(held) | Disposition::Clear(held) => Some(HeldTransition {
                branch: branch.to_string(),
                old: Some(held.clone()),
                new: None,
            }),
            Disposition::Remap(held) => {
                let mut new = held.clone();
                if let At::Commit { id, .. } = &mut new.at
                    && let Some(rewrite) = rewrites.iter().find(|r| r.old == *id)
                {
                    *id = rewrite.new.clone();
                }
                Some(HeldTransition {
                    branch: branch.to_string(),
                    old: Some(held.clone()),
                    new: Some(new),
                })
            }
        }
    }

    /// What the report says for a `Drop`: the verb (`verb_of`) and `onto`
    /// as a person says it — `restack::resolve_onto(repo, onto)?.name`, or
    /// `onto` with `refs/heads/` stripped when the ref is gone.
    pub(crate) fn dropped(&self, repo: &gix::Repository) -> Option<DroppedHold> {
        let Disposition::Drop(held) = self else {
            return None;
        };
        let (Intent::Restack { onto, .. } | Intent::Merge { onto, .. }) = &held.intent else {
            return None;
        };
        Some(DroppedHold {
            verb: verb_of(held),
            onto: onto_short(repo, onto),
        })
    }
}

/// A hold's `onto` as a person says it: `restack::resolve_onto(repo, onto)?
/// .name`, or `onto` with `refs/heads/` stripped when the ref is gone.
pub(crate) fn onto_short(repo: &gix::Repository, onto: &str) -> String {
    match crate::restack::resolve_onto(repo, onto) {
        Ok(base) => base.name,
        Err(_) => onto.strip_prefix("refs/heads/").unwrap_or(onto).to_string(),
    }
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
    replan_at(repo, held, None, None)
}

/// `replan`, with the branch the hold stands on and the open change handed
/// in rather than read off HEAD and the working copy. `ff done` finishing a
/// resolution passes both: HEAD is on the session branch by then, and the
/// working copy holds the fixes, not the change the hold was recorded over.
/// Every other caller passes `None` and the repository answers for itself.
/// A restack and a done name their branch in the intent and ignore `on`.
pub fn replan_at(
    repo: &gix::Repository,
    held: &Held,
    on: Option<&str>,
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
        Intent::Merge { branch, onto, .. } => {
            crate::merge::replan(repo, branch, onto).map_err(|e| expired("merge", e))
        }
        Intent::Done { session } => {
            crate::done::replan_done(repo, session, open).map_err(|e| expired("done", e))
        }
        Intent::Absorb { .. } | Intent::Lift { .. } => {
            let MoveIntent {
                verb,
                from,
                into,
                message,
                paths,
            } = held.intent.as_move().expect("the arm matched a move");
            let verb = verb.as_str();
            let from: Vec<crate::absorb::Endpoint> = from
                .iter()
                .map(|end| crate::absorb::Endpoint::parse(end))
                .collect::<Result<_>>()
                .map_err(|e| expired(verb, e))?;
            let into = crate::absorb::Endpoint::parse(into).map_err(|e| expired(verb, e))?;
            crate::absorb::replan_move(
                repo,
                on,
                &from,
                into,
                paths,
                open,
                message.map(|m| m.as_bytes().into()),
            )
            .map_err(|e| expired(verb, e))
        }
        // The open commit alone, onto the branch's tip as it stands now.
        Intent::Arrive { branch, open } => {
            let open_id = gix::ObjectId::from_hex(open.as_bytes())
                .map_err(|e| expired("switch", Error::msg(e.to_string())))?;
            if repo.try_find_object(open_id).ok().flatten().is_none() {
                return Err(expired(
                    "switch",
                    Error::msg(format!("the parked change {open} is gone")),
                ));
            }
            let tip = refs::ref_target(repo, &format!("refs/heads/{branch}"))
                .ok()
                .flatten()
                .ok_or_else(|| {
                    expired("switch", Error::msg(format!("{branch} no longer exists")))
                })?;
            Ok(Replan {
                target: open_id,
                tip: open_id,
                change: crate::rewrite::Change::Onto(tip),
            })
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
/// is what makes a hold undoable and what `ff op trim` eventually ages out.
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

/// The resolution `branch` is the session of, as `(the branch the hold
/// stands on, its record)`. A branch is a resolution session when its own
/// metadata carries a session whose `onto` names a branch whose `resolving`
/// names it back: both halves have to agree, so a stale record on either
/// side reads as no session rather than as somebody else's.
pub fn session_of(repo: &gix::Repository, branch: &str) -> Result<Option<(String, Resolve)>> {
    let Some(session) = crate::branchmeta::read(repo, branch)?.session else {
        return Ok(None);
    };
    let Some(resolve) = resolving(repo, &session.onto)? else {
        return Ok(None);
    };
    if resolve.session != branch {
        return Ok(None);
    }
    Ok(Some((session.onto, resolve)))
}

/// The refusal for a `resolving` record with no session branch: one written
/// before sessions were branches, whose markers are in the branch's own
/// working copy. Nothing here can land or resume it; `--abandon` clears it
/// and leaves the working copy alone.
pub(crate) fn predates_sessions(branch: &str) -> Error {
    Error::coded(
        "held/expired",
        format!(
            "the resolution on {branch} predates session branches: its markers are in your              working copy"
        ),
        vec!["ff resolve --abandon".into(), "ff restore <path>".into()],
    )
}

/// A park a landing spends rather than brings back: the change was folded
/// into the chain through the record's `open`, so bringing it back would
/// apply it twice. An open commit is cleared off its branch and pinned; a
/// legacy entry is dropped from the stash list the way it always was.
#[derive(Debug, Clone)]
pub struct Spent {
    pub branch: String,
    pub sha: gix::ObjectId,
    pub legacy: bool,
}

impl Spent {
    pub(crate) fn of(repo: &gix::Repository, branch: &str) -> Result<Option<Self>> {
        if let Some(sha) = refs::ref_target(repo, &crate::open::open_ref(branch))? {
            return Ok(Some(Spent {
                branch: branch.to_string(),
                sha,
                legacy: false,
            }));
        }
        Ok(stash::parked_entry(repo, branch)?.map(|sha| Spent {
            branch: branch.to_string(),
            sha,
            legacy: true,
        }))
    }
}

/// The return trip a resolution landing makes: the session branch is
/// deleted, HEAD goes back to the branch the landing leaves you on, the
/// working copy moves from the fixes to the landed tip, and the parked
/// change the way out left behind is either brought back or spent. Planned
/// once by `finish_resolution` and executed by whichever verb lands, inside
/// that verb's own operation, so one `ff undo` takes the landing and the
/// return back together.
#[derive(Debug, Clone)]
pub struct Return {
    /// The session branch, S.
    pub session: String,
    /// S's tip: the marker commit, unless something committed on S.
    pub session_tip: gix::ObjectId,
    pub session_meta: branchmeta::Session,
    /// S's own open commit — the fixes as the last capture wrote them —
    /// cleared with the branch and pinned by the landing.
    pub session_open: Option<gix::ObjectId>,
    /// S's working copy as a tree, the fixes: the from-tree of the landing's
    /// worktree transition.
    pub worktree: gix::ObjectId,
    /// The branch the hold stands on, whose `held` and `resolving` records
    /// the landing clears.
    pub hold_on: String,
    /// The branch HEAD lands on: the held branch for a restack, merge,
    /// absorb, or lift; the branch an editing session lands on for a done.
    pub to: String,
    /// The branch whose parked change comes back on arrival: the held branch
    /// for a restack or merge, the landing branch for a done, none for an
    /// absorb or lift.
    pub arrive_on: Option<String>,
    /// The parked change the landing spends: the held branch's for an absorb
    /// or lift, the editing session's for a done, none for a restack.
    pub drop: Option<Spent>,
}

impl Return {
    /// Plan the trip from `session` for a resolution of `hold_on`'s `intent`,
    /// whose fixes stand in `worktree`.
    pub(crate) fn plan(
        repo: &gix::Repository,
        session: &str,
        hold_on: &str,
        intent: &Intent,
        worktree: gix::ObjectId,
    ) -> Result<Self> {
        let session_tip = refs::ref_target(repo, &format!("refs/heads/{session}"))?
            .ok_or_else(|| Error::msg(format!("internal: the session branch {session} is gone")))?;
        let session_meta = branchmeta::read(repo, session)?
            .session
            .ok_or_else(|| Error::msg(format!("internal: {session} carries no session")))?;
        let session_open = refs::ref_target(repo, &crate::open::open_ref(session))?;
        let (to, arrive_on, drop) = match intent {
            // A restack lands on the held branch, and so does a merge: its
            // park comes home the same way.
            Intent::Restack { branch, .. } | Intent::Merge { branch, .. } => {
                (branch.clone(), Some(branch.clone()), None)
            }
            Intent::Absorb { .. } | Intent::Lift { .. } => {
                let onto = session_meta.onto.clone();
                let drop = Spent::of(repo, &onto)?;
                (onto, None, drop)
            }
            Intent::Done { session: edit } => {
                let landing = branchmeta::read(repo, edit)?
                    .session
                    .ok_or_else(|| Error::msg(format!("internal: {edit} carries no session")))?
                    .onto;
                let drop = Spent::of(repo, edit)?;
                (landing.clone(), Some(landing), drop)
            }
            // An arrival is resolved in place, never on a session.
            Intent::Arrive { .. } => {
                return Err(Error::msg(
                    "internal: a held arrival has no resolution session to return from",
                ));
            }
        };
        Ok(Self {
            session: session.to_string(),
            session_tip,
            session_meta,
            session_open,
            worktree,
            hold_on: hold_on.to_string(),
            to,
            arrive_on,
            drop,
        })
    }

    /// The arrival on `arrive_on` against the tip the landing writes.
    pub(crate) fn arrival(
        &self,
        repo: &gix::Repository,
        new_tip: gix::ObjectId,
        new_tip_tree: gix::ObjectId,
        now: i64,
    ) -> Result<ArrivePlan> {
        match &self.arrive_on {
            Some(branch) => crate::park::plan_arrival(repo, branch, new_tip, new_tip_tree, now),
            None => Ok(ArrivePlan::none()),
        }
    }

    /// Fold the trip into the verb's write-ahead: the HEAD move, the session
    /// branch's deletion and its session's end, the arrival, the spent park,
    /// and the pins. `stash_lines` is the stash reflog as the verb read it,
    /// for the legacy entries.
    pub(crate) fn fold_into(
        &self,
        planned: &mut RefsTable,
        record: &mut OpRecord,
        pins: &mut Vec<gix::ObjectId>,
        stash_lines: &mut Vec<gix::ObjectId>,
        arrive: &ArrivePlan,
    ) {
        let session_ref = format!("refs/heads/{}", self.session);
        let to_ref = format!("refs/heads/{}", self.to);
        planned.head = format!("ref:{to_ref}");
        record.head = Some((format!("ref:{session_ref}"), format!("ref:{to_ref}")));
        planned.refs.remove(&session_ref);
        record.refs.push(RefTransition {
            name: session_ref,
            old: Some(self.session_tip.to_string()),
            new: None,
        });
        record.resolve_session = Some(SessionTransition {
            branch: self.session.clone(),
            old: Some(self.session_meta.clone()),
            new: None,
        });
        pins.push(self.session_tip);
        pins.extend(self.session_open);

        arrive.fold_into(planned, record, pins, stash_lines);
        if let Some(spent) = &self.drop {
            if spent.legacy {
                stash::spend(stash_lines, planned, record, pins, &spent.branch, spent.sha);
            } else {
                pins.push(spent.sha);
            }
        }
    }

    /// Point HEAD at the landing branch. Before the verb's ref transaction,
    /// which deletes the branch HEAD stands on.
    pub(crate) fn leave(&self, repo: &gix::Repository, now: i64) -> Result<()> {
        branch::retarget_head(repo, &format!("refs/heads/{}", self.to), now)
    }

    /// The ref edits the trip adds to the verb's transaction: the session
    /// branch's deletion, and a spent legacy park's ref, so the landing and
    /// the return are all-or-nothing.
    pub(crate) fn edits(&self) -> Result<Vec<gix::refs::transaction::RefEdit>> {
        let mut edits = vec![refs::delete_edit(
            &format!("refs/heads/{}", self.session),
            self.session_tip,
        )?];
        if let Some(spent) = &self.drop
            && spent.legacy
        {
            edits.push(refs::delete_edit(
                &stash::parked_ref(&spent.branch),
                spent.sha,
            )?);
        }
        Ok(edits)
    }

    /// The mutating half, after the refs moved: index and working copy to
    /// the landed tip, the arrival, the spent park, the session's open ref,
    /// and the three metadata records — the session branch's session, and
    /// the hold and the resolution on the branch it stood on. Returns the
    /// arrival and how many files the worktree write touched.
    pub(crate) fn land(
        &self,
        repo: &gix::Repository,
        new_tip_tree: gix::ObjectId,
        arrive: &ArrivePlan,
        now: i64,
    ) -> Result<(crate::model::ArrivalReport, usize)> {
        crate::index::write_index_for_tree(repo, new_tip_tree)?;
        let everything = |_: &str| true;
        let transition =
            crate::worktree::apply_tree_transition(repo, self.worktree, new_tip_tree, &everything)?;
        let files = transition.written.len() + transition.deleted.len();
        let arrival = match &self.arrive_on {
            Some(branch) => crate::park::execute_arrival(repo, branch, arrive, new_tip_tree, now)?,
            None => crate::model::ArrivalReport::None,
        };
        if let Some(spent) = &self.drop {
            if spent.legacy {
                stash::drop_stash_entry(repo, spent.sha)?;
            } else {
                crate::open::clear(repo, &spent.branch, now)?;
            }
        }
        // The session branch is gone with its open ref: the object stays
        // pinned by this operation.
        crate::open::clear(repo, &self.session, now)?;
        // The session branch's file stays, `forked_from` and all: undo puts
        // `session` back from the recorded transition, not from the file.
        let mut meta = branchmeta::read(repo, &self.session)?;
        meta.session = None;
        branchmeta::write(repo, &self.session, &meta)?;
        set(repo, &self.hold_on, None)?;
        set_resolving(repo, &self.hold_on, None)?;
        let _ = crate::futures::cache::remove(repo, &self.session);
        Ok((arrival, files))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rewrite::Rewrite;

    fn hold(at: At) -> Held {
        Held {
            intent: Intent::Restack {
                branch: "feature".into(),
                onto: "refs/heads/main".into(),
            },
            at,
            paths: vec!["f.txt".into()],
            time: 7,
        }
    }

    const OLD: &str = "1111111111111111111111111111111111111111";
    const NEW: &str = "2222222222222222222222222222222222222222";
    const ELSEWHERE: &str = "3333333333333333333333333333333333333333";

    #[test]
    fn a_remap_follows_the_rewrite_map_and_leaves_the_rest() {
        let rewrites = vec![Rewrite {
            old: OLD.into(),
            new: NEW.into(),
        }];
        let at = |id: &str| At::Commit {
            id: id.into(),
            subject: "f1".into(),
        };

        let t = Disposition::Remap(hold(at(OLD)))
            .transition("feature", &rewrites)
            .expect("a remap is recorded");
        assert_eq!(t.branch, "feature");
        assert_eq!(t.old, Some(hold(at(OLD))));
        assert_eq!(t.new, Some(hold(at(NEW))), "at follows the rewrite");

        let t = Disposition::Remap(hold(at(ELSEWHERE)))
            .transition("feature", &rewrites)
            .unwrap();
        assert_eq!(
            t.new,
            Some(hold(at(ELSEWHERE))),
            "an id in no rewrite stays"
        );

        let t = Disposition::Remap(hold(At::OpenChange))
            .transition("feature", &rewrites)
            .unwrap();
        assert_eq!(t.new, Some(hold(At::OpenChange)), "the open change stays");

        let t = Disposition::Drop(hold(at(OLD)))
            .transition("feature", &rewrites)
            .unwrap();
        assert_eq!(t.new, None, "a drop clears");
        let t = Disposition::Clear(hold(at(OLD)))
            .transition("feature", &rewrites)
            .unwrap();
        assert_eq!(t.new, None, "a clear clears");
        assert!(Disposition::None.transition("feature", &rewrites).is_none());
    }

    #[test]
    fn a_remap_onto_the_holds_own_base_is_a_landing() {
        let held = hold(At::OpenChange);
        assert_eq!(
            Disposition::Remap(held.clone()).landing_on("refs/heads/main"),
            Disposition::Clear(held.clone())
        );
        assert_eq!(
            Disposition::Remap(held.clone()).landing_on("refs/heads/other"),
            Disposition::Remap(held.clone())
        );
        assert_eq!(
            Disposition::Drop(held.clone()).landing_on("refs/heads/main"),
            Disposition::Drop(held)
        );
    }
}
