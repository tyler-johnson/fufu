//! One move, two spellings. `ff absorb` and `ff lift` both take content out
//! of a run of commits and land it in one commit: `--from <revset>` names
//! the sources, `--into <rev>` the target, and each verb's word is its
//! defaults and nothing more. Absorb moves from the open change into the
//! commit under the sources; lift moves from the commit under the open
//! change into the open change. Since the open change is a commit, both
//! ends are commits, and the ladder of shapes is one routine:
//!
//! - the target below the sources: the target takes the sources folded in,
//!   and each source is replayed without what moved, so one that empties
//!   is dropped — absorb's default, and git's `rebase -i` squash;
//! - the target above the sources, or among them: the lowest source loses
//!   what moved, everything between replays without it, and the target
//!   takes the moved paths from a fold of whatever stood above it;
//! - the target is the open change: the sources lose what moved and the
//!   worktree stays where it is, so the open change gains it — lift's
//!   default.
//!
//! Sources are a contiguous run on the branch's first-parent line, the open
//! change allowed as the top member or alone; the target is any commit on
//! the line, or the open change. Content moves as whole files, restricted
//! by the path filter; everything between and above replays in one
//! operation, and a conflict holds.
//!
//! Neither spelling writes a single file: nothing is added to or removed
//! from the working tree, and content is only reattributed between commits
//! and the open change. The worktree is byte-identical before and after —
//! only refs, the index, and the operation log move.

use std::collections::{BTreeMap, HashMap};

use gix::bstr::{BString, ByteSlice};

use crate::branchmeta;
use crate::cascade::{self, CascadePlan};
use crate::error::{Error, Result};
use crate::futures::At;
use crate::held::{self, Held, Intent};
use crate::hooks;
use crate::model::{
    Cascade, HeadState, HeldReport, MoveOutcome, MoveReport, MoveSource, MoveTarget,
};
use crate::ops::record::observe_refs;
use crate::ops::{ChangeIdTransition, DescriptionTransition, OpKind, OpRecord, verb};
use crate::park::ArrivePlan;
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::stash;

/// The spelling a move was typed in. The word is the defaults: absorb moves
/// from the open change into the commit under the sources, lift from the
/// commit under the open change into the open change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveVerb {
    Absorb,
    Lift,
}

impl MoveVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            MoveVerb::Absorb => "absorb",
            MoveVerb::Lift => "lift",
        }
    }

    /// The verb with its preposition, for "there is nothing to ___".
    fn noun(self) -> &'static str {
        match self {
            MoveVerb::Absorb => "absorb into",
            MoveVerb::Lift => "lift from",
        }
    }

    /// The past participle, for "nothing was ___".
    fn past(self) -> &'static str {
        match self {
            MoveVerb::Absorb => "absorbed",
            MoveVerb::Lift => "lifted",
        }
    }
}

/// One end of a move, as the caller resolved it: the open change, or a
/// commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Open,
    Commit(gix::ObjectId),
}

impl Endpoint {
    /// The spelling a hold records: `@`, or the full sha.
    pub fn spell(self) -> String {
        match self {
            Endpoint::Open => "@".to_string(),
            Endpoint::Commit(id) => id.to_string(),
        }
    }

    /// The spelling a report reads: `the open change`, or the short sha.
    fn short(self) -> String {
        match self {
            Endpoint::Open => "the open change".to_string(),
            Endpoint::Commit(id) => crate::sha::short_oid(id),
        }
    }

    /// The inverse of [`Endpoint::spell`].
    pub fn parse(text: &str) -> Result<Endpoint> {
        if text == "@" {
            return Ok(Endpoint::Open);
        }
        Ok(Endpoint::Commit(
            gix::ObjectId::from_hex(text.as_bytes()).map_err(|e| Error::msg(e.to_string()))?,
        ))
    }
}

/// What a move was asked to do.
#[derive(Debug, Clone)]
pub struct MoveOptions {
    pub verb: MoveVerb,
    /// `--from`. `None` is the verb's default: absorb the open change, lift
    /// the commit under it.
    pub from: Option<Vec<Endpoint>>,
    /// `--into`. `None` is the verb's default: absorb the commit under the
    /// lowest source, lift the open change.
    pub into: Option<Endpoint>,
    /// The path filter; empty moves whole commits.
    pub paths: Vec<String>,
    /// `-m`: the target's message — a reword for a closed commit, the
    /// pending description for the open change.
    pub message: Option<String>,
    pub verify: hooks::Verify,
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// The branch the rewrite runs on and its tip: `on` when named — a
/// resolution landing names the held branch, since HEAD stands on the
/// session by then — and HEAD's otherwise.
fn head_branch(
    repo: &gix::Repository,
    on: Option<&str>,
    verb_noun: &str,
) -> Result<(String, gix::ObjectId)> {
    if let Some(name) = on {
        let tip = refs::ref_target(repo, &format!("refs/heads/{name}"))?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {name}"),
                vec![],
            )
        })?;
        return Ok((name.to_string(), tip));
    }
    let head = crate::head::head_state(repo)?;
    match head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            Ok((name, tip))
        }
        HeadState::Unborn { .. } => Err(Error::coded(
            "target/unresolvable",
            format!("nothing is committed yet: there is nothing to {verb_noun}"),
            vec!["ff commit -m <msg>".into()],
        )),
        HeadState::Detached { .. } => Err(Error::coded(
            "repo/detached",
            "detached HEAD: there is no branch to carry the rewrite",
            vec!["ff switch <branch>".into()],
        )),
    }
}

/// The tree of a commit, resolved through whichever repository handle is
/// given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// The first parent's tree, or the empty tree for a root commit.
fn parent_tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    let obj = repo.find_object(commit).map_err(Error::repo)?;
    let commit_ref =
        gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => tree_of(repo, gix::ObjectId::from_hex(hex).map_err(Error::repo)?),
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out — an absorb must be exact for the same reason a
/// commit is. The second result says the tree is clean, and the scan comes
/// back with it because the hook window needs the paths a filter left behind.
fn open_tree(
    repo: &gix::Repository,
    tip_tree: gix::ObjectId,
) -> Result<(gix::ObjectId, bool, snaptree::Scan)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true, scan));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false, scan))
}

/// A three-way tree merge, resolved but not yet written: the caller probes
/// with a handle that writes nothing, and only then merges for real.
fn merge_into<'a>(
    repo: &'a gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
    labels: Option<&(String, String)>,
) -> Result<gix::merge::tree::Outcome<'a>> {
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    let labels = match labels {
        Some((ours_label, theirs_label)) => gix::merge::blob::builtin_driver::text::Labels {
            ancestor: None,
            current: Some(ours_label.as_bytes().as_bstr()),
            other: Some(theirs_label.as_bytes().as_bstr()),
        },
        None => Default::default(),
    };
    repo.merge_trees(base, ours, theirs, labels, options)
        .map_err(Error::repo)
}

/// The labels a fold writes when it conflicts. A conflicted fold is handed
/// straight to `chain` as a step's tree, so its markers have to be fufu's
/// own: `regions` and `attribute` only see a block whose closer carries a
/// step, and a block nobody can attribute is a block that lands inside a
/// commit. `owner` is the commit whose step the fold is — the bottom, or a
/// target standing above it — and `k` its position in the chain.
fn fold_labels(
    repo: &gix::Repository,
    bottom: gix::ObjectId,
    tip: gix::ObjectId,
    owner: gix::ObjectId,
    k: usize,
) -> Result<(String, String)> {
    // The size of a stack under a tree change does not depend on the tree,
    // and the fold that produces it is what these labels are for, so the
    // bottom's own tree stands in.
    let change = rewrite::Change::Tree {
        tree: tree_of(repo, bottom)?,
        message: None,
    };
    let n = rewrite::stack_size(repo, bottom, tip, &change)?;
    Ok(rewrite::chain_labels(&subject(repo, owner)?, k, n))
}

/// A rewrite that conflicts is an outcome, not an error: record the hold the
/// caller assembled as a slim operation and report it. Nothing moves — no
/// ref, no file, no futures cache — so the whole path is the operation's
/// append and the branch's metadata, the way `ff describe` records a pending
/// description. The verb travels as an argument because the report and the
/// refusal spell the move the way it was typed.
fn hold(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    verb: MoveVerb,
    branch: &str,
    held: &Held,
    summary: String,
    of: usize,
) -> Result<HeldReport> {
    held::refuse_if_held(repo, branch, verb.past())?;
    held::record(repo, rec, branch, held, summary)?;
    Ok(HeldReport {
        verb: verb.as_str().to_string(),
        branch: branch.to_string(),
        at: held.at.clone(),
        paths: held.paths.clone(),
        of,
    })
}

/// An end of the move that is not on the branch's first-parent line: a
/// commit on another line of work, below a fork since left, or one a
/// later rewrite has moved out of reach — a hold recorded earlier may name
/// exactly that, and re-planning it would move into nothing.
fn not_in_history(id: gix::ObjectId, tip: gix::ObjectId) -> Error {
    Error::coded(
        "rewrite/not-in-history",
        format!(
            "{} is not in the history of {}: there is nothing to rewrite there",
            crate::sha::short_oid(id),
            crate::sha::short_oid(tip)
        ),
        vec!["ff log".into(), "ff log -r <rev>".into()],
    )
}

/// The branch's first-parent line as far down as the move reaches: each
/// commit's depth below the tip, tip at 0. The walk stops once every
/// wanted commit is placed, so a move near the tip of a long history costs
/// what it touches; a wanted commit the walk never meets is off the line.
fn depths(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    wanted: &[gix::ObjectId],
) -> Result<HashMap<gix::ObjectId, i64>> {
    let mut depth: HashMap<gix::ObjectId, i64> = HashMap::new();
    let mut left: usize = wanted.len();
    let mut cursor = Some(tip);
    let mut d = 0i64;
    while let Some(id) = cursor {
        if depth.insert(id, d).is_none() && wanted.contains(&id) {
            left -= 1;
        }
        if left == 0 {
            break;
        }
        let obj = repo.find_object(id).map_err(Error::repo)?;
        let commit_ref =
            gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()).map_err(Error::repo)?;
        cursor = match commit_ref.parents.first() {
            Some(hex) => Some(gix::ObjectId::from_hex(hex).map_err(Error::repo)?),
            None => None,
        };
        d += 1;
    }
    for id in wanted {
        if !depth.contains_key(id) {
            return Err(not_in_history(*id, tip));
        }
    }
    Ok(depth)
}

/// The run a move works on, placed on the branch's line: the sources
/// deepest first, the target, and each end's depth — the open change at
/// −1, the tip at 0.
struct Run {
    /// Deepest first. Never empty, and never holds the target.
    from: Vec<Endpoint>,
    into: Endpoint,
    depth: HashMap<Endpoint, i64>,
}

impl Run {
    fn depth_of(&self, end: Endpoint) -> i64 {
        self.depth[&end]
    }

    /// The deepest source.
    fn lo(&self) -> Endpoint {
        self.from[0]
    }

    /// The shallowest source.
    fn hi(&self) -> Endpoint {
        *self.from.last().expect("a run has a source")
    }

    /// Whether the open change is among the sources.
    fn includes_open(&self) -> bool {
        self.from.contains(&Endpoint::Open)
    }

    /// The commit the rewrite starts at: the deepest of the sources and the
    /// target. The open change is never it — it is the top of the line, and
    /// a move whose deepest end is the open change has the target below.
    fn bottom(&self) -> gix::ObjectId {
        let deepest = self
            .from
            .iter()
            .copied()
            .chain(std::iter::once(self.into))
            .max_by_key(|end| self.depth_of(*end))
            .expect("a run has a source");
        match deepest {
            Endpoint::Commit(id) => id,
            Endpoint::Open => unreachable!("the open change is never the deepest end of a move"),
        }
    }

    /// The sources standing above the target, deepest first.
    fn above_target(&self) -> Vec<Endpoint> {
        let target = self.depth_of(self.into);
        self.from
            .iter()
            .copied()
            .filter(|end| self.depth_of(*end) < target)
            .collect()
    }

    /// The sources as a report or a summary spells them: one short sha, a
    /// range of two, the open change, or a range and the open change.
    fn spell_from(&self) -> String {
        let closed: Vec<Endpoint> = self
            .from
            .iter()
            .copied()
            .filter(|end| *end != Endpoint::Open)
            .collect();
        let run = match (closed.first(), closed.last()) {
            (None, _) | (_, None) => String::new(),
            (Some(lo), Some(hi)) if lo == hi => lo.short(),
            (Some(lo), Some(hi)) => format!("{}..{}", lo.short(), hi.short()),
        };
        match (run.is_empty(), self.includes_open()) {
            (true, _) => "the open change".to_string(),
            (false, false) => run,
            (false, true) => format!("{run} and the open change"),
        }
    }

    /// `from <sources> into <target>`, the body of every line that names
    /// the move.
    fn describe(&self) -> String {
        format!("from {} into {}", self.spell_from(), self.into.short())
    }
}

/// Place the ends on the branch's line and check the run's shape. The
/// target is dropped from the sources — a target inside the run takes both
/// sides — and what remains must be contiguous on the line, the open change
/// allowed as the top member. An end off the first-parent line is refused
/// as not in history.
fn resolve_run(
    repo: &gix::Repository,
    verb: MoveVerb,
    tip: gix::ObjectId,
    from: &[Endpoint],
    into: Endpoint,
) -> Result<Run> {
    let wanted: Vec<gix::ObjectId> = from
        .iter()
        .chain(std::iter::once(&into))
        .filter_map(|end| match end {
            Endpoint::Commit(id) => Some(*id),
            Endpoint::Open => None,
        })
        .collect();
    let placed = depths(repo, tip, &wanted)?;
    let mut depth: HashMap<Endpoint, i64> = HashMap::new();
    depth.insert(Endpoint::Open, -1);
    for (id, d) in placed {
        depth.insert(Endpoint::Commit(id), d);
    }

    let mut sources: Vec<Endpoint> = from.iter().copied().filter(|end| *end != into).collect();
    sources.sort_by_key(|end| std::cmp::Reverse(depth[end]));
    sources.dedup();
    if sources.is_empty() {
        return Err(Error::coded(
            "usage/move-into-self",
            format!(
                "{} is the only source and the target: there is nothing to move",
                into.short()
            ),
            vec![
                match verb {
                    MoveVerb::Absorb => "ff absorb --from <revset> --into <rev>".into(),
                    MoveVerb::Lift => "ff lift --from <revset> --into <rev>".into(),
                },
                "ff log".into(),
            ],
        ));
    }

    // Contiguity: every step down the sorted sources is one commit, or two
    // with the target standing in the gap.
    let target_depth = depth[&into];
    let mut gap = false;
    for pair in sources.windows(2) {
        let (deeper, shallower) = (depth[&pair[0]], depth[&pair[1]]);
        let step = deeper - shallower;
        if step == 1 || (step == 2 && deeper - 1 == target_depth) {
            continue;
        }
        gap = true;
    }
    if gap {
        let lo = sources[0];
        let hi = *sources.last().expect("a run has a source");
        let hi_spelled = match hi {
            Endpoint::Open => "@".to_string(),
            Endpoint::Commit(id) => crate::sha::short_oid(id),
        };
        return Err(Error::coded(
            "usage/move-gap",
            format!(
                "the sources are not one run of commits: {} and {} have commits between them \
                 that were not named",
                lo.short(),
                hi_spelled
            ),
            vec![match verb {
                MoveVerb::Absorb => format!("ff absorb --from {}~..{}", lo.short(), hi_spelled),
                MoveVerb::Lift => format!("ff lift --from {}~..{}", lo.short(), hi_spelled),
            }],
        ));
    }

    Ok(Run {
        from: sources,
        into,
        depth,
    })
}

/// The tree of an end: the open tree for the open change, the commit's own
/// otherwise.
fn tree_of_end(
    repo: &gix::Repository,
    end: Endpoint,
    open_tree: gix::ObjectId,
) -> Result<gix::ObjectId> {
    match end {
        Endpoint::Open => Ok(open_tree),
        Endpoint::Commit(id) => tree_of(repo, id),
    }
}

/// The tree under an end: the tip's for the open change, the first parent's
/// otherwise.
fn parent_tree_of_end(
    repo: &gix::Repository,
    end: Endpoint,
    tip_tree: gix::ObjectId,
) -> Result<gix::ObjectId> {
    match end {
        Endpoint::Open => Ok(tip_tree),
        Endpoint::Commit(id) => parent_tree_of(repo, id),
    }
}

/// A move planned against the repository as it stands: the triple the
/// engine takes, plus what the folds left unresolved, so the verb can hold
/// before the chain runs.
struct MovePlan {
    replan: held::Replan,
    /// The paths the bottom's fold — the target taking the sources — left
    /// conflicted. Empty when the target is not the bottom.
    bottom_conflicts: Vec<String>,
    /// The paths the fold above the target left conflicted. Empty when no
    /// source stands above the target.
    into_conflicts: Vec<String>,
}

/// Plan the move: the run's trees and the [`rewrite::Change::Move`] the
/// engine replays. The folds are computed against the open tree handed in,
/// so a hold re-planned later sees whatever has been done since it was
/// recorded, and a fold that conflicts is returned conflicts and all — what
/// a conflicted fold means is the caller's decision, not the plan's.
fn plan_move(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    run: &Run,
    paths: &[String],
    open_tree: gix::ObjectId,
    message: Option<BString>,
) -> Result<MovePlan> {
    let tip_tree = tree_of(repo, tip)?;
    let bottom = run.bottom();
    let mut bottom_conflicts = Vec::new();
    let mut into_conflicts = Vec::new();

    // Every closed source, replayed without what moved: its own tree with
    // the moved paths reset to its parent's content.
    let mut sources: BTreeMap<gix::ObjectId, gix::ObjectId> = BTreeMap::new();
    for end in &run.from {
        if let Endpoint::Commit(id) = end {
            let lifted =
                rewrite::filtered(repo, tree_of(repo, *id)?, parent_tree_of(repo, *id)?, paths)?;
            sources.insert(*id, lifted);
        }
    }

    let lo = run.lo();
    let hi = run.hi();
    let p_lo_tree = parent_tree_of_end(repo, lo, tip_tree)?;
    let hi_tree = tree_of_end(repo, hi, open_tree)?;

    let change = match run.into {
        // The target below the run: it takes the sources folded in, and the
        // bottom of the rewrite is the target itself. Adjacent, the fold is
        // the run's content over the target's own parent tree; at a distance
        // it is a three-way merge that can leave unresolved paths.
        Endpoint::Commit(target) if run.depth_of(run.into) > run.depth_of(lo) => {
            let theirs = rewrite::filtered(repo, p_lo_tree, hi_tree, paths)?;
            let target_tree = tree_of(repo, target)?;
            let tree = if target_tree == p_lo_tree {
                theirs
            } else {
                let labels = fold_labels(repo, bottom, tip, target, 1)?;
                let mut outcome = merge_into(repo, p_lo_tree, target_tree, theirs, Some(&labels))?;
                bottom_conflicts = crate::futures::unresolved(&outcome);
                outcome.tree.write().map_err(Error::repo)?.detach()
            };
            rewrite::Change::Move {
                tree,
                message,
                sources,
                into: None,
            }
        }
        // The target above the run, or inside it: the bottom is the lowest
        // source, replayed without what moved, and the target takes the
        // moved paths from its own tree with any sources above it folded in
        // up front.
        Endpoint::Commit(target) => {
            let tree = sources
                .remove(&bottom)
                .expect("the lowest source is the bottom, and it is closed");
            let target_tree = tree_of(repo, target)?;
            let above = run.above_target();
            let k = usize::try_from(
                run.depth_of(Endpoint::Commit(bottom)) - run.depth_of(run.into) + 1,
            )
            .expect("the target stands above the bottom");
            let fold = match above.first() {
                None => target_tree,
                Some(lowest_above) => {
                    let p_up_tree = parent_tree_of_end(repo, *lowest_above, tip_tree)?;
                    let theirs = rewrite::filtered(repo, p_up_tree, hi_tree, paths)?;
                    if p_up_tree == target_tree {
                        theirs
                    } else {
                        let labels = fold_labels(repo, bottom, tip, target, k)?;
                        let mut outcome =
                            merge_into(repo, p_up_tree, target_tree, theirs, Some(&labels))?;
                        into_conflicts = crate::futures::unresolved(&outcome);
                        outcome.tree.write().map_err(Error::repo)?.detach()
                    }
                }
            };
            rewrite::Change::Move {
                tree,
                message: None,
                sources,
                into: Some(rewrite::MoveInto {
                    id: target,
                    tree: fold,
                    paths: paths.to_vec(),
                    message,
                }),
            }
        }
        // The open change: the sources lose what moved, the worktree stays
        // where it is, and the open change gains it.
        Endpoint::Open => {
            let tree = sources
                .remove(&bottom)
                .expect("the lowest source is the bottom, and it is closed");
            rewrite::Change::Move {
                tree,
                message: None,
                sources,
                into: None,
            }
        }
    };

    Ok(MovePlan {
        replan: held::Replan {
            target: bottom,
            tip,
            change,
        },
        bottom_conflicts,
        into_conflicts,
    })
}

/// The triple a held move replays, re-derived from the repository as it
/// stands: the same plan the verb built, so the verb and `held::replan`
/// cannot disagree. `open`, when given, is the working tree a resolution
/// session recorded before it wrote the markers over it — the same change
/// this read otherwise takes from disk.
pub(crate) fn replan_move(
    repo: &gix::Repository,
    on: Option<&str>,
    from: &[Endpoint],
    into: Endpoint,
    paths: &[String],
    open: Option<gix::ObjectId>,
    message: Option<BString>,
) -> Result<held::Replan> {
    // The spelling only reaches the refusals' exits, and a replan's are
    // reported as an expiration; absorb stands in.
    let verb = MoveVerb::Absorb;
    let (_branch, tip) = head_branch(repo, on, verb.noun())?;
    let tip_tree = tree_of(repo, tip)?;
    let run = resolve_run(repo, verb, tip, from, into)?;
    let open_tree = match open {
        Some(tree) => tree,
        None => open_tree(repo, tip_tree)?.0,
    };
    Ok(plan_move(repo, tip, &run, paths, open_tree, message)?.replan)
}

/// Move content between commits: `ff absorb` and `ff lift`, one routine.
pub fn move_change(
    repo: &gix::Repository,
    opts: &MoveOptions,
    prov: &Provenance,
) -> Result<(MoveOutcome, verb::VerbContext)> {
    move_with(repo, opts, prov, &rewrite::Decided::none())
}

/// `move_change`, with some rewritten commits' trees decided in advance:
/// those skip the three-way merge and take what they are given, the folds
/// whose results are decided are not probed, and the pre-flight — a
/// question about merges that are no longer going to happen — is asked only
/// when nothing is decided.
pub fn move_with(
    repo: &gix::Repository,
    opts: &MoveOptions,
    prov: &Provenance,
    decided: &rewrite::Decided,
) -> Result<(MoveOutcome, verb::VerbContext)> {
    let verb = opts.verb;
    let paths = &opts.paths;
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            format!("bare repository: nothing to {}", verb.as_str()),
            vec![],
        ));
    }

    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge \
                 --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;
    // A resolution landing runs on the held branch, named by the clearing:
    // HEAD stands on the resolution session, and the return trip is what
    // brings it back.
    let clearing = decided.clearing.as_ref();
    let return_trip = clearing.and_then(|c| c.return_trip.as_ref());
    let on = clearing.map(|c| c.branch.as_str());
    let (branch, tip) = head_branch(repo, on, verb.noun())?;
    let tip_tree = tree_of(repo, tip)?;

    let run = resolve_ends(repo, opts, tip)?;

    // The open change: read off the working copy, or recorded by the
    // resolution session that is landing — `chain` folded the change and
    // the reader's fixes in, so the working copy, which holds the session's
    // fixes rather than the change, is not read at all, and the emptiness
    // refusal cannot fire: the hold was recorded over a change that was not
    // empty.
    let mut open_tree = tip_tree;
    let mut scan = snaptree::Scan::default();
    match clearing
        .and_then(|c| c.resolve.as_ref())
        .and_then(|r| r.open.as_deref())
    {
        Some(hex) => {
            open_tree = gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo)?;
        }
        None if clearing.is_none() => {
            let (read, _clean, read_scan) = self::open_tree(repo, tip_tree)?;
            open_tree = read;
            scan = read_scan;
        }
        None => {}
    }

    // The moved content: for each source, the paths the filter selects
    // among the ones it introduced. None anywhere is nothing to move.
    let mut files = moved_files(repo, &run, paths, tip_tree, open_tree)?;
    if clearing.is_none() && files.is_empty() {
        return Ok((nothing_to_move(verb, &branch), ctx));
    }

    // The pre-commit gate, run only when the open change is a source: that
    // is when worktree content becomes commit content. The staged index is
    // the tip's tree with the selected paths taken from the worktree —
    // precisely the open change's share of what is moving — so a
    // hook-runner asking `git diff --cached` against the tip is told exactly
    // those paths, and a partial move shows it exactly that slice.
    //
    // A resolution landing has already run the gate in `finish_resolution`:
    // this re-entry must not run it a second time.
    let mut window = None;
    if clearing.is_none()
        && run.includes_open()
        && opts.verify == hooks::Verify::Run
        && hooks::will_run(repo, &["pre-commit"])?
    {
        let staged = rewrite::filtered(repo, tip_tree, open_tree, paths)?;
        if staged != tip_tree {
            let differs = snaptree::unselected_paths(&scan, paths);
            let (opened, ran) =
                hooks::Window::open(repo, staged, &differs, opts.verify, verb.as_str())?;
            window = Some(opened);
            if ran {
                // A formatter's fixes are part of what moves, the same way
                // they are part of a close — so re-read the worktree and put
                // the emptiness refusal again, since the hook may have
                // reverted the change it was handed. `self::` because the
                // local binding shadows the helper's name from here on.
                let (reread, _clean, _scan) = self::open_tree(repo, tip_tree)?;
                open_tree = reread;
                files = moved_files(repo, &run, paths, tip_tree, open_tree)?;
                if files.is_empty() {
                    return Ok((nothing_to_move(verb, &branch), ctx));
                }
            }
        }
    }

    let Reword {
        message,
        pending,
        reworded_subject,
    } = reword_for(repo, opts, &run, clearing.is_none())?;

    // The plan. Its folds can conflict before a single descendant is
    // replayed, so when nothing is decided the plan is built first against
    // a handle that writes nothing, and a conflicted fold holds there: the
    // fold cannot apply the open change to the target — `at` is the open
    // change — or cannot fold a closed source into it — `at` is the target —
    // and the move never reaches a replay, so `of` is 0: the size of the
    // stack it would have restacked is unknown here, and we do not invent
    // one. A decided landing has the fold's result in `decided`, so the
    // merge is no longer going to happen and is not probed.
    if decided.is_empty()
        && let Some((at, conflicted)) =
            probe_folds(repo, tip, &run, paths, open_tree, message.clone())?
    {
        return held_outcome(repo, ctx, prov, opts, &branch, &run, at, conflicted, 0);
    }
    let MovePlan { replan, .. } = plan_move(repo, tip, &run, paths, open_tree, message)?;
    let bottom = replan.target;
    let change = replan.change;

    // Pre-flight the descendant replay with the same `change` `plan` will get:
    // a conflict is a hold, and after a clean pre-flight `plan` cannot
    // conflict. Skipped for a decided landing: its trees are already known,
    // so the replay has nothing left to conflict on.
    if decided.is_empty()
        && let Some(conflict) = rewrite::conflict(repo, bottom, tip, &change)?
    {
        return held_outcome(
            repo,
            ctx,
            prov,
            opts,
            &branch,
            &run,
            conflict.at,
            conflict.paths,
            conflict.of,
        );
    }
    let plan = rewrite::plan_with(repo, bottom, tip, &change, now, &decided.trees)?;
    let published = rewrite::published_count(repo, &branch, &plan)?;

    // The branches stacked above. Planned once the new tip is known and
    // before anything is written, so the whole cascade rides this operation
    // and one undo takes it back with the move. A head inside the
    // rewritten range is `plan.carried`'s to move: the cascade reads it as
    // wholly inside its base as it stood, and leaves it to that move.
    let cascade = cascade_after(repo, &branch, tip, plan.new_tip, now)?;

    // Write-ahead: the planned table is the post-move world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let described = run.describe();
    let summary = match cascade.report.moved.len() {
        0 => format!("move {described} on {branch}"),
        n => format!("move {described} on {branch}, and {n} above it"),
    };
    let mut record = OpRecord::new(verb.as_str(), summary, now);
    record.argv = opts.argv.clone();
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = crate::held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }
    // A described open change has an identity from here on, so the `@` row
    // wears the letters its commit will carry. Journaled with the
    // description: an undo takes both back.
    let mut meta = branchmeta::read(repo, &branch)?;
    let mut minted: Option<String> = None;
    if let Some(text) = &pending {
        record.description = Some(DescriptionTransition {
            branch: branch.clone(),
            old: meta.pending_description.clone(),
            new: Some(text.clone()),
        });
        if meta.change_id.is_none() {
            let id = crate::changeid::ChangeId::mint()?.letters();
            record.change_id = Some(ChangeIdTransition {
                branch: branch.clone(),
                old: None,
                new: Some(id.clone()),
                old_born: None,
                new_born: Some(now),
            });
            minted = Some(id);
        }
    }

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    // The return trip rides this record: the HEAD move off the resolution
    // session, that branch's deletion, and the spent park — the change the
    // fold already carries, which an arrival would apply a second time.
    let new_tip_tree = tree_of(repo, plan.new_tip)?;
    if let Some(ret) = return_trip {
        let mut stash_lines = stash::lines(repo)?;
        ret.fold_into(
            &mut planned,
            &mut record,
            &mut pins,
            &mut stash_lines,
            &ArrivePlan::none(),
        );
    }
    // The cascade rides this record: its ref moves, rewrites, drops, and
    // holds, and the planned table says where its branches will stand.
    cascade.fold_into(&mut record, &mut planned, &mut pins);

    // A move writes no files, so the planned worktree is the one already
    // there, and the index is about to be rewritten to match the new tip.
    // A resolution landing is the exception: the working copy moves from
    // the session's fixes to the tip it just wrote.
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: if return_trip.is_some() {
                new_tip_tree
            } else {
                ctx.pre_tree
            },
            index_tree: new_tip_tree,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // HEAD leaves the resolution session, whose branch the transaction below
    // deletes.
    if let Some(ret) = return_trip {
        ret.leave(repo, now)?;
    }
    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("{}: {described}", verb.as_str());
    let mut edits = Vec::new();
    for t in &plan.carried {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            continue;
        };
        let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
        let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
        edits.push(refs::update_edit(
            &t.name,
            new_id,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                old_id,
            )),
            &reflog_msg,
        )?);
    }
    // The branches above move in the same transaction: all of them or none,
    // and the resolution session's deletion with them.
    edits.extend(cascade.edits(&reflog_msg)?);
    if let Some(ret) = return_trip {
        edits.extend(ret.edits()?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                format!(
                    "refs moved while {}; nothing was rewritten (re-run to {} on the new tips)",
                    match verb {
                        MoveVerb::Absorb => "absorbing",
                        MoveVerb::Lift => "lifting",
                    },
                    verb.as_str()
                ),
                vec![],
            ));
        }
    }

    // The cascade's holds onto their branches, now that the refs have moved,
    // and its futures caches. A hold above does not hold the move: the move
    // landed, and the stacked branch waits on its own metadata.
    cascade.land(repo)?;

    // The move has landed: the staged index is no longer provisional, and
    // putting the old one back would contradict the refs that just moved.
    // Every exit before this point — a declining hook, a hold, `ref/contended`,
    // any `?` on the way — drops the window armed and gets the index back
    // byte-for-byte.
    if let Some(window) = window.take() {
        window.landed();
    }

    // A resolution landing is the return trip's: the working copy moves
    // from the session's fixes to the tip just written — the fixes are
    // already in the commits — the park is spent, and the hold and the
    // session it resolved are cleared, so one `ff undo` of this op takes the
    // whole resolution back.
    match return_trip {
        Some(ret) => {
            ret.land(repo, new_tip_tree, &ArrivePlan::none(), now)?;
        }
        None => crate::index::write_index_for_tree(repo, new_tip_tree)?,
    }

    // The open change's description, written once the refs have moved: the
    // landing above may have rewritten the branch's metadata, so it is read
    // again rather than carried across.
    if let Some(text) = pending {
        meta = branchmeta::read(repo, &branch)?;
        meta.pending_description = Some(text);
        if minted.is_some() {
            meta.change_id = minted;
            meta.change_born = Some(now);
        }
        branchmeta::write(repo, &branch, &meta)?;
    }

    let report = move_report(
        repo,
        verb,
        branch,
        &run,
        &plan,
        Landed {
            files,
            published,
            paths: paths.clone(),
            reworded_subject,
            // The exact open tree from above, not `ctx.pre_tree`: the
            // capture floor may have size-capped a blob out of `pre_tree`
            // while the exact tree kept it, and comparing the capped tree
            // would report work still open when there is none. A resolution
            // landing left the tree standing on the new tip, so nothing is
            // open there by construction.
            still_open: run.includes_open() && return_trip.is_none() && open_tree != new_tip_tree,
            cascade: cascade.report,
        },
    )?;
    Ok((MoveOutcome::Moved(Box::new(report)), ctx))
}

/// The ends, defaulted by the verb's word, resolved onto the branch's line,
/// and refused where a bare word would do something it should not. Absorb's
/// default target is the commit under the lowest source, which needs the
/// sources placed first; the placeholder is the tip, and the run is resolved
/// again once the sources are known.
fn resolve_ends(repo: &gix::Repository, opts: &MoveOptions, tip: gix::ObjectId) -> Result<Run> {
    let verb = opts.verb;
    let from: Vec<Endpoint> = match &opts.from {
        Some(from) => from.clone(),
        None => match verb {
            MoveVerb::Absorb => vec![Endpoint::Open],
            MoveVerb::Lift => vec![Endpoint::Commit(tip)],
        },
    };
    let into = match opts.into {
        Some(into) => into,
        None => match verb {
            MoveVerb::Absorb => under_the_sources(repo, verb, tip, &from)?,
            MoveVerb::Lift => Endpoint::Open,
        },
    };
    // The bare words: absorb aimed at the open change is where the changes
    // already are, and lift taking from the open change has nothing
    // committed to take. Any other move whose target is its only source is
    // the generic refusal, raised by `resolve_run`.
    if into == Endpoint::Open && from == [Endpoint::Open] {
        return Err(match (verb, opts.from.is_none(), opts.into.is_none()) {
            (MoveVerb::Absorb, true, _) => Error::coded(
                "usage/absorb-into-open",
                "the open change is already where your changes are: name a commit that has \
                 closed",
                vec!["ff absorb".into(), "ff commit -m <msg>".into()],
            ),
            (MoveVerb::Lift, _, true) => Error::coded(
                "usage/lift-from-open",
                "the open change has nothing committed to lift out of: name a commit that has \
                 closed",
                vec!["ff lift".into(), "ff log".into()],
            ),
            _ => Error::coded(
                "usage/move-into-self",
                "the open change is the only source and the target: there is nothing to move",
                vec![
                    match verb {
                        MoveVerb::Absorb => "ff absorb --from <revset> --into <rev>".into(),
                        MoveVerb::Lift => "ff lift --from <revset> --into <rev>".into(),
                    },
                    "ff log".into(),
                ],
            ),
        });
    }
    let run = resolve_run(repo, verb, tip, &from, into)?;

    // A default target on trunk: the sources sit at the bottom of the
    // branch, and the commit under them is trunk's. Rewriting trunk by
    // default is not a thing a bare word should do, so it is named or
    // nothing. A trunk that cannot be resolved is not consulted.
    if opts.into.is_none()
        && let Endpoint::Commit(target) = run.into
        && run.from.iter().any(|end| *end != Endpoint::Open)
        && let Ok(trunk) = crate::trunk::trunk(repo)
        && let Some(trunk_tip) = refs::ref_target(repo, &trunk.full_ref)?
        && on_line_of(repo, target, trunk_tip)?
    {
        // Only absorb reaches here: lift's default target is the open change.
        let lo = run.lo();
        let exit = if run.from.len() == 1 {
            format!("ff absorb --into {}", lo.short())
        } else {
            let hi_spelled = match run.hi() {
                Endpoint::Open => "@".to_string(),
                Endpoint::Commit(id) => crate::sha::short_oid(id),
            };
            format!(
                "ff absorb --from {}..{} --into {}",
                lo.short(),
                hi_spelled,
                lo.short()
            )
        };
        return Err(Error::coded(
            "absorb/into-trunk",
            format!(
                "the commit under {} is {}, on {}: name the commit to move into",
                lo.short(),
                crate::sha::short_oid(target),
                trunk.name
            ),
            vec![exit, "ff log".into()],
        ));
    }
    Ok(run)
}

/// Nothing selected anywhere: the verb's outcome for a move with no content.
fn nothing_to_move(verb: MoveVerb, branch: &str) -> MoveOutcome {
    MoveOutcome::Nothing {
        verb: verb.as_str().to_string(),
        branch: branch.to_string(),
    }
}

/// What `-m` means for the move, by the target's kind.
struct Reword {
    /// A closed target's new message, through the hooks.
    message: Option<BString>,
    /// The open change's pending description.
    pending: Option<String>,
    /// The target's subject as the report names it: the reword's, when there
    /// is one, since that is what the commit says once it lands.
    reworded_subject: Option<String>,
}

/// `-m`: a reword for a closed target, through the message hooks the way
/// `ff describe <rev>` runs them; the pending description for the open
/// change. A resolution landing (`fresh` false) carries the text the hold
/// recorded, which already went through the hooks.
fn reword_for(
    repo: &gix::Repository,
    opts: &MoveOptions,
    run: &Run,
    fresh: bool,
) -> Result<Reword> {
    let verb = opts.verb;
    let mut reword = Reword {
        message: None,
        pending: None,
        reworded_subject: None,
    };
    let Some(text) = &opts.message else {
        return Ok(reword);
    };
    match run.into {
        Endpoint::Commit(target) => {
            let normalized = crate::close::normalize_message(text);
            let normalized = if fresh {
                let target_hex = target.to_string();
                crate::close::normalize_message(&hooks::message_hooks(
                    repo,
                    &normalized,
                    hooks::MsgSource::Commit(&target_hex),
                    opts.verify,
                    verb.as_str(),
                )?)
            } else {
                normalized
            };
            if normalized.is_empty() {
                return Err(Error::coded(
                    "usage/needs-message",
                    "the commit-msg hook left the description empty; a reworded commit \
                     needs one",
                    vec![match verb {
                        MoveVerb::Absorb => "ff absorb -m <msg>".into(),
                        MoveVerb::Lift => "ff lift -m <msg>".into(),
                    }],
                ));
            }
            reword.reworded_subject = Some(
                normalized
                    .lines()
                    .next()
                    .unwrap_or("(no description)")
                    .to_string(),
            );
            reword.message = Some(normalized.into());
        }
        Endpoint::Open => {
            let text = text.trim_end().to_string();
            if !text.is_empty() {
                reword.pending = Some(text);
            }
        }
    }
    Ok(reword)
}

/// The plan's folds can conflict before a single descendant is replayed, so
/// the plan is built first against a handle that writes nothing. A
/// conflicted fold is where the move holds: the fold cannot apply the open
/// change to the target — `at` is the open change — or cannot fold a closed
/// source into it — `at` is the target.
fn probe_folds(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    run: &Run,
    paths: &[String],
    open_tree: gix::ObjectId,
    message: Option<BString>,
) -> Result<Option<(At, Vec<String>)>> {
    let memory = repo.clone().with_object_memory();
    let probe = plan_move(&memory, tip, run, paths, open_tree, message)?;
    let target_at = || match run.into {
        Endpoint::Commit(id) => At::Commit {
            id: id.to_string(),
            subject: subject(repo, id).unwrap_or_default(),
        },
        Endpoint::Open => At::OpenChange,
    };
    Ok(if !probe.bottom_conflicts.is_empty() {
        Some((
            if run.includes_open() {
                At::OpenChange
            } else {
                target_at()
            },
            probe.bottom_conflicts,
        ))
    } else if !probe.into_conflicts.is_empty() {
        Some((target_at(), probe.into_conflicts))
    } else {
        None
    })
}

/// The move holds at `at` over `conflicted`: the hold is recorded on the
/// branch and the verb returns it. `of` is the size of the stack the replay
/// would have restacked — 0 from the fold probe, where the move never
/// reached a replay and the size is unknown rather than invented.
#[allow(clippy::too_many_arguments)]
fn held_outcome(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    prov: &Provenance,
    opts: &MoveOptions,
    branch: &str,
    run: &Run,
    at: At,
    conflicted: Vec<String>,
    of: usize,
) -> Result<(MoveOutcome, verb::VerbContext)> {
    let verb = opts.verb;
    let now = ctx.now;
    let from: Vec<String> = run.from.iter().map(|end| end.spell()).collect();
    let intent = match verb {
        MoveVerb::Absorb => Intent::Absorb {
            from,
            into: run.into.spell(),
            message: opts.message.clone(),
            paths: opts.paths.clone(),
        },
        MoveVerb::Lift => Intent::Lift {
            from,
            into: run.into.spell(),
            message: opts.message.clone(),
            paths: opts.paths.clone(),
        },
    };
    let held = Held {
        intent,
        at,
        paths: conflicted,
        time: now,
    };
    let report = hold(
        repo,
        held::Recording {
            ctx: &ctx,
            prov,
            argv: opts.argv.clone(),
            now,
        },
        verb,
        branch,
        &held,
        format!("hold {} {}", verb.as_str(), run.describe()),
        of,
    )?;
    Ok((MoveOutcome::Held(report), ctx))
}

/// What the write left behind, for the report.
struct Landed {
    files: Vec<String>,
    published: usize,
    paths: Vec<String>,
    reworded_subject: Option<String>,
    still_open: bool,
    cascade: Cascade,
}

/// The report: each end's new identity — or the fact that the rewrite
/// dropped it — and the branches the plan carried.
fn move_report(
    repo: &gix::Repository,
    verb: MoveVerb,
    branch: String,
    run: &Run,
    plan: &rewrite::RewritePlan,
    landed: Landed,
) -> Result<MoveReport> {
    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = plan
        .carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();

    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop.
    let new_of = |id: gix::ObjectId| -> Result<Option<String>> {
        let hex = id.to_string();
        match plan.rewrites.iter().find(|r| r.old == hex) {
            Some(r) => Ok(Some(r.new.clone())),
            None if plan.dropped.iter().any(|d| d.old == hex) => Ok(None),
            None => Err(Error::msg(format!(
                "{} was not in the rewrite plan",
                crate::sha::short_oid(id)
            ))),
        }
    };
    let mut from_report = Vec::new();
    for end in &run.from {
        from_report.push(match end {
            Endpoint::Open => MoveSource {
                id: "@".to_string(),
                subject: None,
                new: None,
                dropped: false,
            },
            Endpoint::Commit(id) => {
                let new = new_of(*id)?;
                MoveSource {
                    id: id.to_string(),
                    subject: Some(subject(repo, *id)?),
                    dropped: new.is_none(),
                    new,
                }
            }
        });
    }
    let into_report = match run.into {
        Endpoint::Open => MoveTarget {
            id: "@".to_string(),
            new: None,
            subject: None,
        },
        Endpoint::Commit(id) => MoveTarget {
            id: id.to_string(),
            new: new_of(id)?,
            subject: Some(match landed.reworded_subject {
                Some(reworded) => reworded,
                None => subject(repo, id)?,
            }),
        },
    };
    // The ends are in `rewrites` exactly when they survived, so they are
    // subtracted from the restack exactly when they are there.
    let ends_rewritten = from_report.iter().filter(|s| s.new.is_some()).count()
        + usize::from(into_report.new.is_some());

    Ok(MoveReport {
        verb: verb.as_str().to_string(),
        branch,
        from: from_report,
        into: into_report,
        files: landed.files,
        restacked: plan.rewrites.len().saturating_sub(ends_rewritten),
        moved,
        published: landed.published,
        paths: landed.paths,
        still_open: landed.still_open,
        dropped: plan.dropped.clone(),
        cascade: landed.cascade,
    })
}

/// Absorb's default target: the commit under the lowest source. The
/// sources are placed on the line for it, and placed again with the target
/// by `resolve_run`; the second walk is a map lookup deep.
fn under_the_sources(
    repo: &gix::Repository,
    verb: MoveVerb,
    tip: gix::ObjectId,
    from: &[Endpoint],
) -> Result<Endpoint> {
    let wanted: Vec<gix::ObjectId> = from
        .iter()
        .filter_map(|end| match end {
            Endpoint::Commit(id) => Some(*id),
            Endpoint::Open => None,
        })
        .collect();
    let placed = depths(repo, tip, &wanted)?;
    let lowest = from
        .iter()
        .copied()
        .max_by_key(|end| match end {
            Endpoint::Open => -1,
            Endpoint::Commit(id) => placed[id],
        })
        .ok_or_else(|| {
            Error::coded(
                "usage/revset-empty-set",
                "--from names no revision",
                vec!["ff log".into()],
            )
        })?;
    match lowest {
        Endpoint::Open => Ok(Endpoint::Commit(tip)),
        Endpoint::Commit(id) => {
            let obj = repo.find_object(id).map_err(Error::repo)?;
            let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash())
                .map_err(Error::repo)?;
            match commit_ref.parents.first() {
                Some(hex) => Ok(Endpoint::Commit(
                    gix::ObjectId::from_hex(hex).map_err(Error::repo)?,
                )),
                None => Err(Error::coded(
                    "target/unresolvable",
                    format!(
                        "{} is the root commit: there is nothing under it to {}",
                        crate::sha::short_oid(id),
                        verb.noun()
                    ),
                    vec![match verb {
                        MoveVerb::Absorb => "ff absorb --into <rev>".into(),
                        MoveVerb::Lift => "ff lift --into <rev>".into(),
                    }],
                )),
            }
        }
    }
}

/// Whether `commit` is on the history of `tip`.
fn on_line_of(repo: &gix::Repository, commit: gix::ObjectId, tip: gix::ObjectId) -> Result<bool> {
    Ok(repo
        .merge_bases_many(commit, &[tip])
        .map_err(Error::repo)?
        .into_iter()
        .any(|id| id.detach() == commit))
}

/// The paths whose content the move carries: for each source, the paths
/// the filter selects among the ones it introduced, sorted and deduped.
fn moved_files(
    repo: &gix::Repository,
    run: &Run,
    paths: &[String],
    tip_tree: gix::ObjectId,
    open_tree: gix::ObjectId,
) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for end in &run.from {
        let own = tree_of_end(repo, *end, open_tree)?;
        let under = parent_tree_of_end(repo, *end, tip_tree)?;
        let selected = rewrite::filtered(repo, under, own, paths)?;
        files.extend(rewrite::changed_paths(repo, under, selected)?);
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// The branches stacked above `branch`, planned once its rewrite is known:
/// the plan the caller folds into its own operation, adds to its one ref
/// transaction, and lands after the refs move. `ff absorb`, `ff lift`, and
/// `ff describe <rev>` share it, and all three run on the branch HEAD
/// stands on, so HEAD is never above the branch being rewritten and the
/// cascade moves no file. A tip that did not move has nothing above it to
/// follow. `ff done` calls it too, from the branch it landed on: HEAD
/// stands on the session branch there, which is nobody's child.
pub(crate) fn cascade_after(
    repo: &gix::Repository,
    branch: &str,
    old_tip: gix::ObjectId,
    new_tip: gix::ObjectId,
    now: i64,
) -> Result<CascadePlan> {
    if new_tip == old_tip {
        return Ok(CascadePlan::default());
    }
    cascade::plan(repo, branch, old_tip, new_tip, None, now)
}
