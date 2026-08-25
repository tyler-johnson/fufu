//! Watching the operation log: given where a subscriber last was, what has
//! the log done since?
//!
//! The naive model is that the log is append-only, so a watcher walks from
//! the last id it saw to the new tip and calls everything in between new.
//! That model is wrong seven distinct ways, and this module exists mostly to
//! be honest about them. An undo moves the ref *backwards* — and appends one
//! or two operations onto the side it is abandoning before it does. `ff op
//! restore` across a fork lands on an id that is neither ancestor nor
//! descendant. New work after an undo is a sibling. A trim deletes the ref,
//! replays what survives, and **rewrites every surviving id**, so the whole
//! address space a subscriber holds stops resolving. A trim with nothing
//! surviving leaves no ref at all. And a reconcile that finds the tip
//! unreadable parks the log and starts a fresh parentless root.
//!
//! So a watcher is not told what was *added*. It is told what the log *did*,
//! and [`Motion`] is the closed set of answers.
//!
//! Two facts shape the order the tests below run in. Auto-trim rides
//! `Lanes::READ`, so an ordinary `ff status` in another terminal — or an
//! agent's capture hook — can rewrite the entire log underneath a live
//! watch. And both the trim path and the reconcile-nuke path park the old
//! tip to the same ref — this worktree's own `refs/fufu/wt/<id>/trash/@ops`,
//! reached through [`OpLog::trash_tip`] rather than named here. That one ref
//! moving is therefore a single sufficient signal for "everything you hold is
//! gone", which is why it is tested first: the operations trim replays would
//! otherwise read as a burst of ordinary appends.
//!
//! It has to be *this* chain's trash ref rather than any chain's, or one
//! worktree's trim would end another worktree's stream. That is why a
//! rewrite is terminal per *chain* rather than per stream: [`Chains`] emits
//! it for the chain it happened to, re-anchors that chain from the tip the
//! event carries, and keeps every other chain running.
//!
//! Two readers, then. [`classify`] answers for the worktree it is called in;
//! [`Chains`] answers for every chain in the repository, one [`Event`] per
//! motion with the chain named on it. Both go through [`classify_in`], which
//! is the same seven rules against a chain named rather than assumed.
//!
//! **Landmine: ancestry here is the `fufu-prev` chain and nothing else.** An
//! operation commit's parent 2 is the base commit — a real commit from the
//! user's own history — so `merge_bases_many`, `is_ancestor`, or any walk
//! over the commit graph leaves the operation log and wanders into the code
//! history. Every walk below follows the message trailer, bounded, the way
//! [`crate::ops::index`]'s catch-up does.
//!
//! One caveat this module inherits rather than introduces: operations are
//! written **write-ahead**, before the mutation they describe. An operation
//! reported here is therefore a claim about the immediate future, not an
//! observation of the past. `ff op log` shows the same operation with the
//! same caveat, so nothing is degraded by streaming it — and nothing is
//! fixed by a field or a delay, either.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::model::OpEntry;
use crate::ops::{OpId, OpKind, OpLog, Operation};

/// How far back either walk will look before giving up and calling it a
/// rewrite. The same value, for the same reason, as
/// `ops::index::CATCHUP_CAP`: generous next to any plausible gap — a few
/// operations per command, a burst of captures from an agent — and still a
/// bound, so a damaged or adversarial log cannot turn one tick into a walk
/// of the whole history.
const WALK_CAP: usize = 4096;

/// What the operation log did.
///
/// Tagged the way [`crate::futures::Verdict`] is tagged, because a
/// subscriber branches on the tag and nothing else: the set is closed, so
/// the wording lives in one place and the JSON stays stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "motion", rename_all = "kebab-case")]
pub enum Motion {
    /// The anchor a stream opens on, before anything has moved. Always the
    /// first thing a subscriber is told, so it has an id to hold.
    Start { tip: Option<OpId> },
    /// An operation was appended.
    Landed {
        /// Boxed because `OpEntry` is ten fields, and without the box every
        /// other variant would be that size too.
        op: Box<OpEntry>,
    },
    /// The log's pointer moved backwards — an undo, or a redo that lands
    /// behind where the watcher was. `over` is how many operations it
    /// stepped past, not how many it wrote: undo appends onto the side it
    /// abandons, and those never reach a subscriber.
    SteppedBack { tip: Option<OpId>, over: usize },
    /// The log left the line the watcher was on and started another. `from`
    /// is where the two still agree; `op` is the first operation on the new
    /// side, and whatever follows it arrives as ordinary [`Motion::Landed`].
    Forked { from: OpId, op: Box<OpEntry> },
    /// The address space is gone. Terminal: every id the subscriber holds
    /// stops resolving here, so this is the end of a stream rather than an
    /// event within one.
    Rewritten { reason: Rewrite, tip: Option<OpId> },
}

/// Why the address space is gone. Two causes, one signal — see the module
/// doc on why the trash ref covers both of the first kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rewrite {
    /// The old log was parked to the trash ref: a trim rewrote every
    /// surviving id, or a reconcile found the tip unreadable and started
    /// over.
    Trim,
    /// No shared history with what the subscriber held, and nothing parked.
    Reset,
}

/// One tick's answer, and what to remember for the next one.
///
/// Both anchors ride the answer rather than being read again by the caller:
/// a watcher that re-read the tip after classifying would be reading a
/// different instant than the one it just described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub motion: Vec<Motion>,
    /// The live tip now — the next call's `last_seen`.
    pub tip: Option<OpId>,
    /// The trash tip now — the next call's `last_trash`.
    pub trash: Option<OpId>,
}

/// One line of a repository-wide stream: what a chain did, and which chain.
///
/// The field rides both modes. A single-tree stream always says the same
/// worktree, which costs a subscriber nothing and buys it one struct to
/// parse. It is an added field rather than a changed one, so the envelope
/// contract stays where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// The chain's id — a live worktree's, or the one a retired worktree
    /// still answers to. `main` for the main worktree.
    pub worktree: String,
    #[serde(flatten)]
    pub motion: Motion,
}

/// A predicate over one arriving operation.
///
/// Deliberately not [`crate::revset::opspace`]: `evaluate_ops` resolves a
/// *set* against a *universe*, which is a different question from "does this
/// one operation belong on this stream", and reaching for it here would drag
/// a whole evaluation in to answer a two-field comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    kind: Option<OpKind>,
    session: Option<String>,
}

impl Filter {
    /// `kind` is one of `capture`, `op`, `foreign`, `note`; anything else is
    /// refused here rather than silently matching nothing. `session` matches
    /// the tag an operation wears, exactly.
    pub fn new(kind: Option<&str>, session: Option<String>) -> Result<Filter> {
        let kind = match kind {
            None => None,
            Some(name) => Some(OpKind::from_str(name).ok_or_else(|| {
                Error::coded(
                    "usage/bad-value",
                    format!(
                        "unknown operation kind {name:?}: expected capture, op, foreign, or note"
                    ),
                    vec!["ff watch --kind op".into()],
                )
            })?),
        };
        Ok(Filter { kind, session })
    }

    fn admits(&self, op: &Operation<'_>) -> bool {
        if self.kind.is_some_and(|want| op.kind() != want) {
            return false;
        }
        match &self.session {
            Some(want) => op.session() == Some(want.as_str()),
            None => true,
        }
    }
}

/// Classify what the log did since `last_seen`, and hand back what to
/// remember for the next call.
///
/// Reads the repository and writes nothing — in particular it never calls
/// [`crate::ops::reconcile`], which appends: a watch that appended would be
/// observing motion it had caused.
pub fn classify(
    repo: &gix::Repository,
    last_seen: Option<OpId>,
    last_trash: Option<OpId>,
    filter: &Filter,
) -> Result<Watched> {
    classify_in(
        repo,
        &crate::ops::chain_id(repo),
        last_seen,
        last_trash,
        filter,
    )
}

/// The same question asked of a chain named rather than assumed.
///
/// [`classify`] is this against the chain the caller is standing in. The
/// seven rules below do not know which chain they are on: they read
/// [`OpLog::tip`], [`OpLog::trash_tip`], and the `fufu-prev` trailer chain,
/// and the operation id space those resolve against is repository-wide
/// already.
pub fn classify_in(
    repo: &gix::Repository,
    chain: &str,
    last_seen: Option<OpId>,
    last_trash: Option<OpId>,
    filter: &Filter,
) -> Result<Watched> {
    let log = OpLog::open_chain(repo, chain)?;
    // Both refs, once, at the top: everything below describes this instant.
    let trash = log.trash_tip()?;
    let tip = log.tip()?;

    let settled = |motion: Vec<Motion>| Ok(Watched { motion, tip, trash });

    // 0. No anchor. Nothing has been seen, so nothing can have moved — this
    //    is how a subscriber establishes where it is, and every rule below
    //    may assume there is somewhere to have moved from.
    let Some(last_seen) = last_seen else {
        return settled(Vec::new());
    };

    // 1. The trash tip moved. First, because it is the only motion that
    //    invalidates what came before: a trim rewrites every surviving id,
    //    so the operations it replayed would otherwise be mistaken for a
    //    burst of new appends.
    if trash != last_trash {
        return settled(vec![Motion::Rewritten {
            reason: Rewrite::Trim,
            tip,
        }]);
    }

    // 2. The live tip is gone: a trim with nothing surviving, or a deleted
    //    ref. Nothing was parked, so there is nothing to point a subscriber
    //    at.
    let Some(tip_id) = tip else {
        return settled(vec![Motion::Rewritten {
            reason: Rewrite::Reset,
            tip: None,
        }]);
    };

    // 3. Nothing moved.
    if tip_id == last_seen {
        return settled(Vec::new());
    }

    // 4. The new tip descends from the anchor: an ordinary append, or a run
    //    of them.
    if let Some(above) = walk_to(&log, tip_id, last_seen)? {
        let ids = oldest_first(above);
        return settled(landed(&log, repo, ids, filter)?);
    }

    // 5. The anchor descends from the new tip: the pointer moved back. The
    //    filter does not apply — a pointer move is not an operation.
    if let Some(above) = walk_to(&log, last_seen, tip_id)? {
        return settled(vec![Motion::SteppedBack {
            tip,
            over: above.len(),
        }]);
    }

    // 6. Neither descends from the other, but the two still agree somewhere:
    //    the log left the watcher's line and started another.
    if let Some((from, above)) = fork_point(&log, tip_id, last_seen)? {
        let mut ids = oldest_first(above);
        // The fork's own first operation is emitted whatever the filter
        // says: it names a structural fact about the log rather than a row
        // the filter is selecting. An empty run cannot happen here (rule 5
        // would have caught it) except when a walk hit the cap, and a fork
        // we cannot see the near side of is a reset, not a fork.
        if !ids.is_empty() {
            let first = ids.remove(0);
            let mut motion = vec![Motion::Forked {
                from,
                op: Box::new(one_row(repo, first)?),
            }];
            motion.extend(landed(&log, repo, ids, filter)?);
            return settled(motion);
        }
    }

    // 7. No shared ancestor within the bound.
    settled(vec![Motion::Rewritten {
        reason: Rewrite::Reset,
        tip,
    }])
}

/// Every chain a repository-wide stream is watching, and where each of them
/// was last seen.
///
/// The map only ever grows. A chain it has not seen is anchored and
/// announced — which is the seed on the first tick and an `ff worktree add`
/// on any later one, through one code path rather than two. A chain whose
/// worktree is retired stays, because `ff worktree remove` captures into
/// that chain before the directory goes: dropping it at a tick boundary
/// would race the last thing it ever said.
///
/// Discovery reads live worktrees rather than [`crate::ops::chain_ids`], so
/// a repository with fifty dead bays does not open with fifty anchor lines.
/// Nothing appends to a chain nobody stands in, so nothing is missed —
/// except a chain that died while watched, which the rule above keeps.
#[derive(Debug, Clone, Default)]
pub struct Chains {
    /// Chain id to its `(tip, trash)` anchors, the two things
    /// [`classify_in`] measures from. Ordered so the anchor burst and every
    /// later tick come out the same way twice.
    seen: BTreeMap<String, (Option<OpId>, Option<OpId>)>,
}

impl Chains {
    pub fn new() -> Self {
        Chains::default()
    }

    /// One pass over every chain: what each of them did since last time.
    ///
    /// A chain not yet in the map is classified with no anchors, which is
    /// rule 0 — no motion, and the fresh anchors to remember — so the
    /// [`Motion::Start`] announcing it is synthesized here.
    ///
    /// Both anchors are stored whatever came back, which is what makes
    /// [`Motion::Rewritten`] self-healing: the chain re-anchors to the tip
    /// the event already carried, and the next tick measures from there.
    ///
    /// One corner of that costs a tick. A trim with nothing surviving leaves
    /// no ref at all, so the chain re-anchors on `None` — and `None` is rule
    /// 0 again, which reports no motion. Whatever lands in the tick after
    /// such a rewrite is therefore not reported, and the tick after that
    /// measures from the fresh tip. The subscriber was told the address
    /// space was gone and that there was nothing there, which is exactly
    /// what a stream picking up from an emptied log has to say.
    pub fn tick(&mut self, repo: &gix::Repository, filter: &Filter) -> Result<Vec<Event>> {
        let mut chains: Vec<String> = self.seen.keys().cloned().collect();
        chains.extend(crate::linked::worktree_ids(repo)?);
        chains.sort();
        chains.dedup();

        let mut events = Vec::new();
        for chain in chains {
            let anchors = self.seen.get(&chain).copied();
            let (last_seen, last_trash) = anchors.unwrap_or((None, None));
            let seen = classify_in(repo, &chain, last_seen, last_trash, filter)?;
            if anchors.is_none() {
                events.push(Event {
                    worktree: chain.clone(),
                    motion: Motion::Start { tip: seen.tip },
                });
            }
            events.extend(seen.motion.into_iter().map(|motion| Event {
                worktree: chain.clone(),
                motion,
            }));
            self.seen.insert(chain, (seen.tip, seen.trash));
        }
        Ok(events)
    }
}

/// Walk back from `from` along `fufu-prev`, `from` included, until `target`
/// is reached. `Some` is what lay strictly above `target`, newest first;
/// `None` means the cap was hit or the chain ran out first — never an error,
/// because a bound reached is an answer.
///
/// The shape is `ops::index::catch_up`'s, and for the same reason: a tip
/// mismatch is nearly always a small gap, and paying for the gap beats
/// paying for the log.
fn walk_to(log: &OpLog<'_>, from: OpId, target: OpId) -> Result<Option<Vec<OpId>>> {
    let mut above = Vec::new();
    let mut cur = Some(from);
    while let Some(id) = cur {
        if id == target {
            return Ok(Some(above));
        }
        if above.len() >= WALK_CAP {
            return Ok(None);
        }
        above.push(id);
        cur = log.get(id)?.prev();
    }
    Ok(None)
}

/// Where the chain behind `from` rejoins the chain behind `other`, with
/// whatever lies strictly above the join on `from`'s side, newest first.
fn fork_point(log: &OpLog<'_>, from: OpId, other: OpId) -> Result<Option<(OpId, Vec<OpId>)>> {
    let mut theirs: HashSet<OpId> = HashSet::new();
    let mut cur = Some(other);
    while let Some(id) = cur {
        if theirs.len() >= WALK_CAP {
            break;
        }
        theirs.insert(id);
        cur = log.get(id)?.prev();
    }

    let mut above = Vec::new();
    let mut cur = Some(from);
    while let Some(id) = cur {
        if theirs.contains(&id) {
            return Ok(Some((id, above)));
        }
        if above.len() >= WALK_CAP {
            return Ok(None);
        }
        above.push(id);
        cur = log.get(id)?.prev();
    }
    Ok(None)
}

fn oldest_first(mut newest_first: Vec<OpId>) -> Vec<OpId> {
    newest_first.reverse();
    newest_first
}

/// The rows for `ids`, in the order given, minus what the filter refuses.
fn landed(
    log: &OpLog<'_>,
    repo: &gix::Repository,
    ids: Vec<OpId>,
    filter: &Filter,
) -> Result<Vec<Motion>> {
    let mut admitted = Vec::new();
    for id in ids {
        if filter.admits(&log.get(id)?) {
            admitted.push(id);
        }
    }
    Ok(rows(repo, admitted)?
        .into_iter()
        .map(|op| Motion::Landed { op: Box::new(op) })
        .collect())
}

fn one_row(repo: &gix::Repository, id: OpId) -> Result<OpEntry> {
    rows(repo, vec![id])?
        .pop()
        .ok_or_else(|| Error::msg(format!("operation {id} vanished between walk and read")))
}

/// The same rows `ff op log --json` emits, in the order handed in.
///
/// Reused rather than hand-built so a subscriber learns one shape — and so
/// `short_id` gets the abbreviation length the log view computes, instead of
/// a second answer to the same question.
fn rows(repo: &gix::Repository, ids: Vec<OpId>) -> Result<Vec<OpEntry>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    crate::ops::verb::read_ops_of(repo, ids.into_iter().map(Ok), 0, true)
}
