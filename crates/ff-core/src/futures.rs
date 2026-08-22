//! Futures: what an operation would cost, answered before you spend it. A
//! rebase is a replay, so fufu simulates one — every commit of
//! `base..branch_tip` re-applied onto a moving cursor as an in-memory
//! three-way tree merge — and reports where it would land.
//!
//! The whole replay happens inside one object-memory clone of the
//! repository. Intermediate trees are written there and read back by the
//! next step, and the clone is dropped when the answer is returned, so a
//! probe writes nothing: a repository that has never run one is
//! byte-identical to one that has run a thousand.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// How many commits a replay will simulate before giving up honestly.
const DEFAULT_FUTURES_DEPTH: usize = 200;

/// What a rebase onto the base would do, right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Verdict {
    /// The base is already an ancestor: nothing to replay.
    UpToDate { ahead: usize },
    /// The branch is an ancestor of the base: no replay, just a pointer move.
    FastForward { behind: usize },
    /// Every commit replays without conflict.
    Clean {
        replayed: usize,
        /// Commits the replay would drop as empty; `replayed` does not
        /// include them. Defaulted so a cache file written before the field
        /// existed still deserializes.
        #[serde(default)]
        dropped: usize,
    },
    /// The replay stops here.
    Conflict { at: At, paths: Vec<String> },
    /// Honest silence — a wrong verdict is worse than none.
    Unknown { reason: UnknownReason },
    /// The tracking ref is configured but the ref is not there, and
    /// something says a copy once stood there — the shared copy was deleted.
    /// Only ever produced for a remote.
    Gone,
    /// The tracking ref is configured, the ref is not there, and nothing
    /// says a copy ever was: a clone of an empty remote wears exactly this
    /// shape, and calling it `Gone` reported a loss on a repository's first
    /// status line. Only ever produced for a remote.
    Unpublished,
    /// The shared copy stands exactly where this branch last published it,
    /// and the branch has since stepped back from that tip. What is out
    /// there is yours and undone, not somebody else's work arriving — and
    /// `ff publish` is what rolls it back. Only ever produced for a remote.
    Undone { behind: usize },
}

/// Where a conflict lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "what", rename_all = "kebab-case")]
pub enum At {
    Commit { id: String, subject: String },
    OpenChange,
}

/// Why fufu declined to answer. The set is closed, so the wording lives in
/// one place and the JSON stays stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownReason {
    UnrelatedHistories,
    MergeCommits,
    TooManyCommits,
}

impl UnknownReason {
    /// The phrase the human line prints inside "can't simulate (…)".
    pub fn text(self) -> &'static str {
        match self {
            UnknownReason::UnrelatedHistories => "unrelated histories",
            UnknownReason::MergeCommits => "merge commits in the range",
            UnknownReason::TooManyCommits => "too many commits",
        }
    }
}

/// What a branch answers to, and which of the two nouns names it. Both are
/// restacks; only the wording differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// The base beneath this branch, and the one a reader would assume.
    Trunk,
    /// The base beneath this branch: a branch it was explicitly stacked on.
    Parent,
    /// The shared copy of this same branch.
    Remote,
    /// A tracking ref wearing another branch's name.
    RemoteAlias,
}

impl Role {
    /// Whether this role is a base rather than a remote.
    pub fn is_base(self) -> bool {
        matches!(self, Role::Trunk | Role::Parent)
    }
}

/// Which branch a future is measured against, and how fufu picked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRef {
    /// Short name as a person would say it: `main`, `origin/main`.
    pub name: String,
    pub r#ref: String,
    /// The empty string when the ref is configured but absent — a gone remote.
    /// Every other role always resolves.
    pub tip: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Future {
    pub against: SyncRef,
    pub verdict: Verdict,
}

/// The conflicting paths of a three-way merge, probed entirely in memory.
pub(crate) fn conflict_paths(
    repo: &gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<Vec<String>> {
    let memory = repo.clone().with_object_memory();
    let options = memory.tree_merge_options().map_err(Error::repo)?;
    let outcome = memory
        .merge_trees(base, ours, theirs, Default::default(), options)
        .map_err(Error::repo)?;
    Ok(unresolved(&outcome))
}

/// The unresolved paths of a completed merge, sorted and deduped.
pub(crate) fn unresolved(outcome: &gix::merge::tree::Outcome<'_>) -> Vec<String> {
    let how = gix::merge::tree::TreatAsUnresolved::git();
    let mut paths: Vec<String> = outcome
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(how))
        .map(|c| c.changes_in_resolution().1.location().to_string())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// The tree of a commit, resolved through whichever repository handle is given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// Simulate rebasing `branch_tip` onto `onto`, commit by commit. When
/// `open_tree` is given, one final step covers reapplying uncommitted work.
pub fn probe(
    repo: &gix::Repository,
    onto: gix::ObjectId,
    branch_tip: gix::ObjectId,
    open_tree: Option<gix::ObjectId>,
) -> Result<Verdict> {
    let depth = repo
        .config_snapshot()
        .integer("fufu.futuresDepth")
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_FUTURES_DEPTH);
    probe_to_depth(repo, onto, branch_tip, open_tree, depth)
}

/// As [`probe`], with the replay depth handed in instead of read from
/// `fufu.futuresDepth`: the cap exists because status probes at prompt rate,
/// and a verb the user asked for pays the real cost instead.
pub fn probe_to_depth(
    repo: &gix::Repository,
    onto: gix::ObjectId,
    branch_tip: gix::ObjectId,
    open_tree: Option<gix::ObjectId>,
    depth: usize,
) -> Result<Verdict> {
    // All merge bases, not just the best one: with a criss-cross history a
    // single base misreads both sides.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(branch_tip, &[onto])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Ok(Verdict::Unknown {
            reason: UnknownReason::UnrelatedHistories,
        });
    }

    // Order matters: when the tips are equal both arms below match, and the
    // truthful answer is "up to date", not a fast-forward of nothing.
    if bases.contains(&onto) {
        let ahead = crate::upstream::count_exclusive(repo, branch_tip, &bases)?;
        return Ok(Verdict::UpToDate { ahead });
    }
    if bases.contains(&branch_tip) {
        let behind = crate::upstream::count_exclusive(repo, onto, &bases)?;
        return Ok(Verdict::FastForward { behind });
    }

    // Newest-first from the walk; reversed below. Merge commits bail, so the
    // range is a simple chain and reversing is exactly oldest-first.
    let walk = repo
        .rev_walk(Some(branch_tip))
        .with_boundary(bases.iter().copied())
        .all()
        .map_err(Error::repo)?;
    let mut range: Vec<gix::ObjectId> = Vec::new();
    for info in walk {
        let info = info.map_err(Error::repo)?;
        if info.parent_ids().count() > 1 {
            // Rebase semantics for a merge are ambiguous, and a wrong verdict
            // is worse than an honest silence.
            return Ok(Verdict::Unknown {
                reason: UnknownReason::MergeCommits,
            });
        }
        range.push(info.id);
        if range.len() > depth {
            // Bail on the cap, not after the walk: a branch with fifty
            // thousand commits must cost the cap, not the branch.
            return Ok(Verdict::Unknown {
                reason: UnknownReason::TooManyCommits,
            });
        }
    }
    range.reverse();

    let memory = repo.clone().with_object_memory();
    let options = memory.tree_merge_options().map_err(Error::repo)?;

    let mut cursor = tree_of(repo, onto)?;
    let mut replayed = 0usize;
    let mut dropped = 0usize;

    for id in &range {
        let commit = memory.find_object(*id).map_err(Error::repo)?.into_commit();
        let their_tree = commit.tree_id().map_err(Error::repo)?.detach();
        let base_tree = match commit.parent_ids().next() {
            Some(parent) => tree_of(&memory, parent.detach())?,
            None => gix::ObjectId::empty_tree(memory.object_hash()),
        };
        let mut outcome = memory
            .merge_trees(
                base_tree,
                cursor,
                their_tree,
                Default::default(),
                options.clone(),
            )
            .map_err(Error::repo)?;
        let paths = unresolved(&outcome);
        if !paths.is_empty() {
            let subject = commit.message().map_err(Error::repo)?.summary().to_string();
            return Ok(Verdict::Conflict {
                at: At::Commit {
                    id: id.to_string(),
                    subject,
                },
                paths,
            });
        }
        let merged = outcome.tree.write().map_err(Error::repo)?.detach();
        // A replay that adds nothing over the cursor is a commit the writing
        // half would not write at all: it introduces nothing, so it counts as
        // dropped, not replayed.
        if merged == cursor {
            dropped += 1;
        } else {
            replayed += 1;
        }
        cursor = merged;
    }

    // The open change: one more step, so the verdict covers reapplying work
    // that was never committed.
    if let Some(open) = open_tree {
        let tip_tree = tree_of(repo, branch_tip)?;
        if open != tip_tree {
            let mut outcome = memory
                .merge_trees(tip_tree, cursor, open, Default::default(), options.clone())
                .map_err(Error::repo)?;
            let paths = unresolved(&outcome);
            if !paths.is_empty() {
                return Ok(Verdict::Conflict {
                    at: At::OpenChange,
                    paths,
                });
            }
            let _ = outcome.tree.write().map_err(Error::repo)?;
        }
    }

    Ok(Verdict::Clean { replayed, dropped })
}

/// The tree the operation log last stated for `branch` — the open change as
/// it stands. `None` in a bare repository, or before the log has an entry
/// for this branch.
pub fn open_tree(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    if repo.workdir().is_none() {
        return Ok(None);
    }
    let log = crate::ops::OpLog::open(repo)?;
    match log.branch_tip(branch)? {
        Some(id) => Ok(Some(log.get(id)?.tree())),
        None => Ok(None),
    }
}

/// Which branch `branch` should be measured against. `None` when fufu cannot
/// honestly name one.
pub fn base_for(repo: &gix::Repository, branch: &str) -> Result<Option<SyncRef>> {
    // An editing session sits below the branch it will land on by
    // construction: "behind, fast-forwards" is a permanent condition of
    // being a session, the way being ahead is one — sync never merges the
    // branch into its base, so the pending state is the branch's condition,
    // not work to do. The axis is silenced, not answered.
    if crate::branchmeta::read(repo, branch)?.session.is_some() {
        return Ok(None);
    }

    // 1. An explicitly recorded parent, when it still resolves — by ref and
    // displayed by name, because a parent may be someone else's branch and a
    // tracking ref is where that one lives. Hardcoding `refs/heads/` here
    // would drop it silently to trunk, the same shape as the trunk bug this
    // file already carries a paragraph about.
    //
    // One parent is refused rather than reported: the base axis and the
    // remote axis must never aim at the same ref. That is reachable only
    // when a minted branch is later renamed onto the name it forked from —
    // `ff start origin/x` then `ff describe -b x` — whose own tracking ref is
    // then the parent it recorded.
    let meta = crate::branchmeta::read(repo, branch)?;
    if let Some(parent) = meta.parent.filter(|p| p != branch)
        && let Some((full_ref, tip)) = crate::refs::branchish(repo, &parent)?
        && !remote_for(repo, branch)?.is_some_and(|own| own.r#ref == full_ref)
    {
        return Ok(Some(SyncRef {
            name: parent,
            r#ref: full_ref,
            tip: tip.to_string(),
            role: Role::Parent,
        }));
    }

    // 2. Trunk is the base unless it is the branch underfoot, in which case
    // there is no base at all: trunk sits on nothing. Identity is by name and
    // never by ref: a trunk that lives only on the remote is spelled
    // `refs/remotes/origin/main` while the branch underfoot is
    // `refs/heads/main`, so comparing refs calls them two different branches
    // and hands `main` its own shared copy as a base — one ref wearing both
    // nouns, which every report then says twice. For a local trunk the two
    // comparisons are the same test. Ambiguity is swallowed to None and never
    // propagated: a repository that cannot name its trunk still gets a
    // working `ff status`.
    let trunk = crate::trunk::trunk(repo).ok();
    let standing_on_trunk = trunk.as_ref().is_some_and(|t| t.name == branch);
    if let Some(t) = trunk.as_ref()
        && !standing_on_trunk
        && let Some(tip) = crate::refs::ref_target(repo, &t.full_ref)?
    {
        return Ok(Some(SyncRef {
            name: t.name.clone(),
            r#ref: t.full_ref.clone(),
            tip: tip.to_string(),
            role: Role::Trunk,
        }));
    }

    // Standing on trunk, detached, unborn, or an unresolvable trunk: no
    // honest base. On trunk, the thing it answers to is its remote — now its
    // own axis.
    Ok(None)
}

/// The shared copy of `branch` — the tracking ref git would fetch into.
/// `None` when no upstream is configured.
pub fn remote_for(repo: &gix::Repository, branch: &str) -> Result<Option<SyncRef>> {
    let own_ref = format!("refs/heads/{branch}");
    let full: gix::refs::FullName = own_ref.as_str().try_into().map_err(Error::repo)?;
    let Some(tracking) =
        repo.branch_remote_tracking_ref_name(full.as_ref(), gix::remote::Direction::Fetch)
    else {
        return Ok(None);
    };
    let tracking = tracking.map_err(Error::repo)?;
    let name = tracking.as_ref().shorten().to_string();
    let r#ref = tracking.as_ref().as_bstr().to_string();

    // The remote-side branch name from `branch.<name>.merge`: when it is
    // another branch's name, this tracking ref is an alias, not the branch's
    // own copy. A branch whose upstream cannot be named otherwise is
    // overwhelmingly its own copy, and guessing "alias" would put a wrong
    // name on the screen.
    let role = match repo.branch_remote_ref_name(full.as_ref(), gix::remote::Direction::Fetch) {
        Some(Ok(remote_name)) if remote_name.as_ref().shorten() != branch => Role::RemoteAlias,
        _ => Role::Remote,
    };

    // Asymmetry with `base_for`, on purpose: an unresolvable base is a base
    // fufu cannot name, so `base_for` returns None; an unresolvable remote is
    // a *fact about the remote* — the shared copy was deleted — and the user
    // needs to hear it, so the empty tip is returned, not swallowed.
    let tip = match crate::refs::ref_target(repo, &r#ref)? {
        Some(t) => t.to_string(),
        None => String::new(),
    };

    Ok(Some(SyncRef {
        name,
        r#ref,
        tip,
        role,
    }))
}

/// The futures cache: plain JSON keyed by its own inputs, so a stale entry is
/// by definition one that will not be used. Deleting the directory changes no
/// answer, only the cost of getting it.
pub mod cache {
    use super::*;

    /// One axis's cached verdict, keyed by its own four inputs, so a stale
    /// entry is by definition one that will not be used.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub(super) struct Entry {
        pub(super) against_ref: String,
        pub(super) against_tip: String,
        pub(super) branch_tip: String,
        pub(super) open_tree: Option<String>,
        pub(super) verdict: Verdict,
    }

    /// Both axes in one file: two independent slots, so recomputing one never
    /// costs the other.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub(super) struct File {
        #[serde(default)]
        pub(super) base: Option<Entry>,
        #[serde(default)]
        pub(super) remote: Option<Entry>,
    }

    pub(super) fn path(repo: &gix::Repository, branch: &str) -> PathBuf {
        repo.common_dir().join("fufu/futures").join(branch)
    }

    /// Read the file, replace one slot, write it back — so writing the base
    /// slot never drops a remote slot computed a moment earlier.
    pub(super) fn store(repo: &gix::Repository, branch: &str, is_base: bool, entry: Entry) {
        let path = path(repo, branch);
        let mut file = match crate::jsonfile::read::<File>(&path) {
            Ok(Some(file)) => file,
            _ => File::default(),
        };
        if is_base {
            file.base = Some(entry);
        } else {
            file.remote = Some(entry);
        }
        // Best-effort: a cache that cannot be written must never fail a read.
        let _ = crate::jsonfile::write(&path, &file);
    }

    /// Drop a branch's cached future. Losing it costs recomputation, nothing else.
    pub fn remove(repo: &gix::Repository, branch: &str) -> Result<()> {
        crate::jsonfile::remove(&path(repo, branch))
    }
}

/// Both axes at once. Either may be `None`: a branch standing on trunk has no
/// base, a branch nobody has pushed has no remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Futures {
    pub base: Option<Future>,
    pub remote: Option<Future>,
    /// True when remotes exist and none can be named for this branch — the
    /// state `ff sync` refuses on. It distinguishes `remote: None` meaning
    /// *there is no remote axis, and that is fine* from `remote: None`
    /// meaning *there is something to say and fufu declined to guess*.
    #[serde(default)]
    pub remote_unnamed: bool,
}

/// The future of `branch` along one axis, served from cache when all four
/// inputs still match and computed otherwise. `None` when the branch has no
/// tip yet. The cache-key self-invalidation that made this safe for a single
/// slot is now true twice over: each slot keys on its own four inputs.
fn future_on(
    repo: &gix::Repository,
    branch: &str,
    against: SyncRef,
    branch_tip: Option<gix::ObjectId>,
    open_tree: Option<gix::ObjectId>,
    is_base: bool,
) -> Result<Option<Future>> {
    let Some(tip) = branch_tip else {
        return Ok(None);
    };

    let tip_hex = tip.to_string();
    let open_hex = open_tree.map(|id| id.to_string());

    if let Ok(Some(file)) = crate::jsonfile::read::<cache::File>(&cache::path(repo, branch)) {
        let entry = if is_base { file.base } else { file.remote };
        if let Some(entry) = entry
            && entry.against_ref == against.r#ref
            && entry.against_tip == against.tip
            && entry.branch_tip == tip_hex
            && entry.open_tree == open_hex
        {
            return Ok(Some(Future {
                against,
                verdict: entry.verdict,
            }));
        }
    }

    let against_tip = gix::ObjectId::from_hex(against.tip.as_bytes()).map_err(Error::repo)?;
    let verdict = probe(repo, against_tip, tip, open_tree)?;

    cache::store(
        repo,
        branch,
        is_base,
        cache::Entry {
            against_ref: against.r#ref.clone(),
            against_tip: against.tip.clone(),
            branch_tip: tip_hex,
            open_tree: open_hex,
            verdict: verdict.clone(),
        },
    );

    Ok(Some(Future { against, verdict }))
}

/// What replaying `branch` onto its base would cost.
pub fn base_future(
    repo: &gix::Repository,
    branch: &str,
    branch_tip: Option<gix::ObjectId>,
    open_tree: Option<gix::ObjectId>,
) -> Result<Option<Future>> {
    let Some(against) = base_for(repo, branch)? else {
        return Ok(None);
    };
    future_on(repo, branch, against, branch_tip, open_tree, true)
}

/// What reconciling `branch` with its shared copy would cost.
pub fn remote_future(
    repo: &gix::Repository,
    branch: &str,
    branch_tip: Option<gix::ObjectId>,
    open_tree: Option<gix::ObjectId>,
) -> Result<Option<Future>> {
    let Some(against) = remote_for(repo, branch)? else {
        return Ok(None);
    };
    // A configured-but-absent tracking ref has nothing to simulate against
    // and nothing worth remembering: the answer is the fact itself. Which
    // fact it is turns on evidence — `git clone` of an empty remote writes
    // `branch.<n>.merge` and creates no `refs/remotes/*`, so "deleted" and
    // "never created" wear the same shape and only one of them is a loss.
    if against.tip.is_empty() {
        let verdict = if ever_copied(repo, &against, branch)? {
            Verdict::Gone
        } else {
            Verdict::Unpublished
        };
        return Ok(Some(Future { against, verdict }));
    }
    // The tracking tip is one this branch published and the branch has
    // stepped back from it: the commits out there are yours, undone, and
    // sync would take them straight back in. Answered before the cache the
    // way `Gone` is — and safely, because the cache keys on the same pair
    // this reads, `against_tip` and `branch_tip`.
    if let Some(tip) = branch_tip
        && let Some(behind) = undone_behind(repo, &against, branch, tip)?
    {
        return Ok(Some(Future {
            against,
            verdict: Verdict::Undone { behind },
        }));
    }
    future_on(repo, branch, against, branch_tip, open_tree, false)
}

/// Whether anything says this branch ever had a copy on this remote, and how
/// many commits back: any ref under `refs/remotes/<remote>/` (a clone of a
/// non-empty remote always has some) or the log's own memory of a push.
/// Checked in that order, cheapest first.
fn ever_copied(repo: &gix::Repository, against: &SyncRef, branch: &str) -> Result<bool> {
    if let Some(remote) = remote_of(&against.r#ref)
        && crate::refs::any_remote_ref(repo, remote)?
    {
        return Ok(true);
    }
    crate::published::ever_published(repo, branch)
}

/// The commits the shared copy holds and this branch does not, when that
/// copy stands exactly where the branch last published it. `None` when the
/// branch is not behind it, or when the log has no such row.
fn undone_behind(
    repo: &gix::Repository,
    against: &SyncRef,
    branch: &str,
    tip: gix::ObjectId,
) -> Result<Option<usize>> {
    let against_tip = gix::ObjectId::from_hex(against.tip.as_bytes()).map_err(Error::repo)?;
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(tip, &[against_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    let behind = crate::upstream::count_exclusive(repo, against_tip, &bases)?;
    if behind == 0 {
        return Ok(None);
    }
    Ok(crate::published::published_tip(repo, branch, &against.tip)?.then_some(behind))
}

/// The remote's name out of a tracking ref: `refs/remotes/origin/x` → `origin`.
fn remote_of(r#ref: &str) -> Option<&str> {
    r#ref
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(remote, _)| remote)
}

/// Both axes, for the callers that report both.
pub fn futures_for(
    repo: &gix::Repository,
    branch: &str,
    branch_tip: Option<gix::ObjectId>,
    open_tree: Option<gix::ObjectId>,
) -> Result<Futures> {
    let remote = remote_future(repo, branch, branch_tip, open_tree)?;
    let remote_ref = remote.as_ref().map(|r| r.against.r#ref.as_str());
    // One ref is one axis. A branch can be configured to track the very ref
    // that is also its base — `branch.<name>.merge` pointing at trunk — and
    // then both axes name the same commits. Reporting it twice tells a person
    // there are two things to reconcile when there is one. The remote is the
    // half that survives, because it is the noun that also decides the push.
    let base = match base_for(repo, branch)? {
        Some(base) if Some(base.r#ref.as_str()) == remote_ref => None,
        Some(base) => future_on(repo, branch, base, branch_tip, open_tree, true)?,
        None => None,
    };
    // A branch with a remote future has its remote named by definition, so
    // the flag can only ever be true when the axis came back empty.
    let remote_unnamed = remote.is_none()
        && matches!(
            crate::remote::for_branch(repo, branch),
            crate::remote::RemoteChoice::Ambiguous { .. }
        );
    Ok(Futures {
        base,
        remote,
        remote_unnamed,
    })
}
