//! The plain, serializable read model. No gix types leak out of ff-core:
//! object ids are lowercase hex strings, paths are strings.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HeadState {
    /// HEAD points at a branch that has no commits yet.
    Unborn { r#ref: String },
    /// HEAD is on a branch.
    Branch {
        /// Short branch name, e.g. `feat/x/y`.
        name: String,
        /// Full ref name, e.g. `refs/heads/feat/x/y`.
        r#ref: String,
        commit: String,
    },
    /// HEAD points directly at a commit.
    Detached { commit: String },
}

/// A git operation in progress (a rebase, a merge, a bisect), mirroring
/// `gix::state::InProgress`. Named for what git calls it, because in fufu's
/// own vocabulary an "operation" is an entry in the op log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InProgress {
    ApplyMailbox,
    ApplyMailboxRebase,
    Bisect,
    CherryPick,
    CherryPickSequence,
    Merge,
    Rebase,
    RebaseInteractive,
    Revert,
    RevertSequence,
}

impl From<gix::state::InProgress> for InProgress {
    fn from(state: gix::state::InProgress) -> Self {
        use gix::state::InProgress as S;
        match state {
            S::ApplyMailbox => InProgress::ApplyMailbox,
            S::ApplyMailboxRebase => InProgress::ApplyMailboxRebase,
            S::Bisect => InProgress::Bisect,
            S::CherryPick => InProgress::CherryPick,
            S::CherryPickSequence => InProgress::CherryPickSequence,
            S::Merge => InProgress::Merge,
            S::Rebase => InProgress::Rebase,
            S::RebaseInteractive => InProgress::RebaseInteractive,
            S::Revert => InProgress::Revert,
            S::RevertSequence => InProgress::RevertSequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Upstream {
    /// Short tracking ref name, e.g. `origin/main`.
    pub r#ref: String,
    /// The upstream is configured but its ref no longer exists.
    pub gone: bool,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    TypeChange,
    Renamed,
    Copied,
    IntentToAdd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusEntry {
    pub path: String,
    /// Source path for renames and copies.
    pub from: Option<String>,
    pub kind: ChangeKind,
}

/// One file's contribution to the open change, as `ff status` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileStat {
    pub path: String,
    /// Source path for renames and copies.
    pub from: Option<String>,
    pub kind: ChangeKind,
    pub insertions: u32,
    pub deletions: u32,
    /// Line counts are meaningless — both counts are 0.
    pub binary: bool,
}

/// The open change as a diffstat: HEAD's tree against the capture chain tip's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeStat {
    pub files: Vec<FileStat>,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub head: HeadState,
    pub operation: Option<InProgress>,
    pub upstream: Option<Upstream>,
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,
    pub untracked: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub id: String,
    pub short_id: String,
    pub subject: String,
    pub author_name: String,
    pub author_email: String,
    /// Author time, seconds since the unix epoch.
    pub time: i64,
}

/// One capture operation, as the timeline views render it.
///
/// Still called a snapshot on this surface because that is what it is: a
/// snapshot is what an operation carries, and `ff evolog` shows exactly the
/// operations that carry nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapEntry {
    /// Raw hex — this row names a commit for `git show` as much as an
    /// operation for `ff restore --at`, and the letters spelling is minted
    /// from it at the display edge.
    pub id: String,
    pub short_id: String,
    pub subject: String,
    /// Committer time, seconds since the unix epoch.
    pub time: i64,
    /// The HEAD commit the capture was taken on (base edge), if any.
    pub base: Option<String>,
    /// The previous capture on this branch, if any.
    pub prev: Option<String>,
}

/// What a restore pulled from. Restore reaches into both address spaces —
/// a commit under `--from`, an operation under `--at-op` and `--at` — so the
/// row says which one it was rather than leaving a reader to infer it from
/// the shape of an id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreOrigin {
    /// `commit` or `operation`.
    pub space: String,
    /// Raw hex, always: the model stays hex and the letters spelling is
    /// minted at the display edge.
    pub id: String,
    /// The abbreviation to display — a plain 7 for a commit, the unique
    /// letters prefix for an operation.
    pub short_id: String,
    pub subject: String,
    /// Committer time, seconds since the unix epoch.
    pub time: i64,
}

/// The open change: the working tree summarized as one row (jj's `@`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenChange {
    /// Chain/branch name, or `@detached`.
    pub branch: String,
    /// Chain tip snapshot id (hex), when the chain exists.
    pub id: Option<String>,
    /// The HEAD commit (hex); `None` when unborn.
    pub base: Option<String>,
    pub base_short: Option<String>,
    /// The pending description, when one is set.
    pub subject: Option<String>,
    /// Chain tip committer time, seconds since the unix epoch.
    pub time: Option<i64>,
    /// The tip tree equals the HEAD tree (or no chain exists yet).
    pub clean: bool,
    /// The pending change's stable identity: the hash of the commit the close
    /// would mint — not a prediction (the real close re-stamps time, and hooks
    /// may rewrite tree or message). `None` when nothing is pending or no
    /// user identity is configured.
    pub pending: Option<String>,
}

/// The result of a worktree restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    /// Where the files came from, in the address space that named it.
    pub origin: RestoreOrigin,
    /// Files written (created or overwritten) from the target.
    pub restored: Vec<String>,
    /// Files deleted because the target does not contain them.
    pub deleted: Vec<String>,
    /// Gitlinks (embedded repositories) present in the diff but not touched.
    pub skipped_gitlinks: Vec<String>,
    /// The mandatory pre-restore capture, spelled as an operation id.
    pub pre_op: Option<String>,
}

/// Per-branch view of one trim pass. The counts are that branch's share of
/// the one log — there is no per-branch chain to drop a suffix of any more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrimPointer {
    /// The branch pointer, e.g. `refs/fufu/snap/main`.
    pub r#ref: String,
    /// The branch name (or `@detached`).
    pub branch: String,
    pub dropped: usize,
    pub kept: usize,
    /// Where the pre-trim log tip was saved, when anything was dropped.
    pub trash_ref: Option<String>,
    /// True when nothing of this branch survived and its pointer was deleted.
    pub deleted: bool,
}

/// The one log's retention within a trim pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrimLog {
    pub dropped: usize,
    pub kept: usize,
    /// Where the pre-trim log tip was saved, when anything was dropped.
    pub trash_ref: Option<String>,
    /// True when the whole log expired and the ref was deleted (the next
    /// invocation bootstraps a fresh floor).
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrimReport {
    /// One row per branch pointer into the log. The counts are that
    /// branch's share of the one log's retention, not a chain of its own —
    /// there is no per-branch chain to drop a suffix of any more.
    pub pointers: Vec<TrimPointer>,
    /// The one log's retention; `None` when no log exists yet.
    pub log: Option<TrimLog>,
    /// True when nothing was written (dry run).
    pub dry_run: bool,
}

/// The result of a move along the log: `ff undo`, `ff redo`, `ff op restore`.
/// One mechanism, so one report — the three differ in how the landing was
/// chosen and in nothing that happens afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewindReport {
    /// The operation landed on, in the letters alphabet. This is the state
    /// the repository now holds, not the thing that was undone: the log's
    /// pointer moved here, and everything reported below describes getting
    /// the world to agree with it.
    pub landed: String,
    pub landed_summary: String,
    /// `capture`, `op`, `foreign` or `note`.
    pub landed_kind: String,
    /// How many operations the move stepped over, captures included.
    pub stepped: usize,
    /// What was stepped over, named by the newest operation among them that
    /// somebody decided on — the thing a person means by "what was undone".
    /// A run of captures names its own newest. `None` when nothing was
    /// stepped over at all.
    pub stepped_summary: Option<String>,
    pub stepped_kind: Option<String>,
    /// How many of those were verb operations — decisions somebody made,
    /// rather than the capture floor's own rows. Reported separately because
    /// "undid one close and two other things" is a lie when the two others
    /// were captures that changed no ref.
    pub stepped_ops: usize,
    /// How many operations a run collapsed into this one step. Zero when the
    /// landing was named rather than found, since naming one is not a run.
    /// A keystroke that moved forty operations must not have to be inferred.
    pub collapsed: usize,
    /// Whether the move went forward along the log (`ff redo`).
    pub forward: bool,
    /// Every ref moved, with old and new values.
    pub refs: Vec<crate::ops::RefTransition>,
    /// Where HEAD went, when it moved.
    pub head_moved: Option<String>,
    /// Worktree files written or deleted.
    pub files: Vec<String>,
    pub warnings: Vec<String>,
    pub pre_op: Option<String>,
}

/// The result of `ff op revert` — the one verb in the `ff op` family that
/// writes an operation rather than moving to one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevertReport {
    /// The operation whose change was inverted, in the letters alphabet.
    pub reverted: String,
    pub reverted_summary: String,
    /// The inverse transitions that were applied.
    pub refs: Vec<crate::ops::RefTransition>,
    pub pre_op: Option<String>,
}

/// The result of `ff describe` (pending-description edit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DescribeReport {
    pub branch: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// The result of `ff describe <rev>`: a reword, and the restack it forced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewordReport {
    /// The branch the reword ran on.
    pub branch: String,
    /// The target commit before the reword.
    pub old: String,
    /// The target commit after it.
    pub new: String,
    /// The new subject — the first line of the new message.
    pub subject: String,
    /// Descendants re-parented behind the target.
    pub restacked: usize,
    /// Other local branches carried with the rewrite, short names, sorted.
    pub moved: Vec<String>,
    /// How many of the rewritten commits the branch's remote already has.
    pub published: usize,
}

/// The result of `ff absorb`: the open change — or the part of it a path
/// filter selected — folded into a commit at a distance, and the restack it
/// forced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsorbOutcome {
    /// The change was folded into the target and restacked.
    Absorbed(AbsorbReport),
    /// The replay conflicts, so nothing was written and the rewrite is
    /// waiting. A hold is an outcome and not an error — something happened,
    /// and it has a report — even though the caller still exits 3, because a
    /// human decision is required before anything moves.
    Held(HeldReport),
    /// A clean tree, or a path filter that selected nothing.
    NothingToAbsorb { branch: String },
}

/// An absorb that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AbsorbReport {
    /// The branch the absorb ran on.
    pub branch: String,
    /// The target commit before the absorb.
    pub into: String,
    /// The target commit after it. `None` when the rewrite dropped the
    /// commit — it introduces nothing now, and fufu writes no empty commit —
    /// in which case it is named in `dropped`.
    pub new: Option<String>,
    /// The target's subject, which an absorb never changes.
    pub subject: String,
    /// Descendants restacked behind the target.
    pub restacked: usize,
    /// Other local branches carried with the rewrite, short names, sorted.
    pub moved: Vec<String>,
    /// How many of the rewritten commits the branch's remote already has.
    pub published: usize,
    /// The paths the filter selected; empty means the whole open change.
    pub paths: Vec<String>,
    /// Whether anything is still open once the absorb has landed.
    pub still_open: bool,
    /// Commits the rewrite dropped because they introduce nothing — fufu
    /// writes no empty commit. Oldest-first.
    pub dropped: Vec<crate::rewrite::Dropped>,
}

/// The result of `ff restack`: a branch's commits replayed onto a different
/// base, the open change carried onto the new tip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestackOutcome {
    /// The branch was replayed onto its base. Boxed because the landed
    /// report dwarfs the other variant, and an enum sized to its largest
    /// arm is paid for on every return, landed or not.
    Restacked(Box<RestackReport>),
    /// Already sitting on its base, and no re-aim was asked for.
    NothingToRestack { branch: String, base: String },
    /// The replay conflicts, so nothing was written and the rewrite is
    /// waiting. A hold is an outcome and not an error — something happened,
    /// and it has a report — even though the caller still exits 3, because a
    /// human decision is required before anything moves.
    Held(HeldReport),
}

/// A restack that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestackReport {
    /// The branch that moved.
    pub branch: String,
    /// The base it was replayed onto, as a person would say it: `main`.
    pub base: String,
    /// That base's tip commit, full sha.
    pub onto: String,
    /// `--onto` recorded a new parent for this branch.
    pub reaimed: bool,
    /// The parent it recorded before, when there was one.
    pub previous_parent: Option<String>,
    /// Commits replayed. Zero on a fast-forward and on a bare re-aim.
    pub replayed: usize,
    /// How many commits the base holds that the branch did not — what
    /// "the base moved" amounts to.
    pub behind: usize,
    /// The base already contained the branch, so the ref moved and nothing
    /// was rewritten.
    pub fast_forward: bool,
    /// The branch's tip after the restack, full sha.
    pub new_tip: String,
    /// Other local branches carried with the rewrite, short names, sorted.
    pub moved: Vec<String>,
    /// How many of the rewritten commits the branch's remote already has.
    pub published: usize,
    /// The tracking ref `published` was measured against — `origin/feature`.
    /// `None` when the branch has no upstream. Restack can rewrite a branch
    /// you are not standing on, so this cannot be re-derived from HEAD.
    pub published_on: Option<String>,
    /// The branch has a parked change, and whether it would still apply.
    pub parked: Option<Parked>,
    /// Worktree files written or deleted. Zero when the restacked branch is
    /// not the one HEAD stands on or under.
    pub files: usize,
    /// Anything still open once the restack has landed.
    pub still_open: bool,
    /// Commits the rewrite dropped because they introduce nothing — fufu
    /// writes no empty commit. Oldest-first.
    pub dropped: Vec<crate::rewrite::Dropped>,
}

/// A parked change the restack leaves untouched, disclosed not resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parked {
    /// The stash commit fufu recorded for that branch.
    pub stash: String,
    /// It still merges onto the branch's new tip.
    pub applies: bool,
}

/// The result of `ff edit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditOutcome {
    /// A session was opened on a commit.
    Opened(EditReport),
    /// The target named a branch, so the verb was a switch. A kind mismatch
    /// redirects rather than refuses: `ff edit` targets commits, `ff switch`
    /// targets branches, and one available reading is taken and announced.
    Switched(SwitchReport),
}

/// An editing session that opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditReport {
    /// The anonymous branch minted for the session — the session *is* a branch.
    pub session: String,
    /// The commit being edited, full sha. It is the session branch's own tip:
    /// travel happens in ref-space, so nothing is held back.
    pub editing: String,
    pub subject: String,
    /// The branch whose commits wait ahead, and replay when the session ends.
    pub onto: String,
    /// How many of its commits wait ahead.
    pub ahead: usize,
    /// The stash sha the open change parked under, when the tree was dirty.
    pub parked: Option<String>,
}

/// The result of `ff resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ResolveOutcome {
    /// The conflicts are in the working tree, waiting for you.
    Opened(ResolveReport),
    /// The rewrite no longer conflicts, so there was nothing to resolve: the
    /// hold is released and the verb that recorded it will land it.
    Released(ReleasedReport),
    /// The hold was dropped.
    Abandoned(AbandonedHold),
}

/// A resolution session that opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolveReport {
    pub branch: String,
    /// The verb that held.
    pub verb: String,
    /// Files carrying conflict markers, sorted.
    pub files: Vec<String>,
    /// How many marked regions are waiting.
    pub regions: usize,
    /// How many steps the chain ran, and how many the rewrite has in all —
    /// equal unless a tangle stopped it short.
    pub steps: usize,
    pub of: usize,
    /// The commit the chain stopped before, when two conflicts landed on one
    /// region and the markers would have interleaved rather than nested.
    pub tangled: Option<String>,
    /// The change parked to make room, when the tree was dirty.
    pub parked: Option<String>,
}

/// A hold released because the rewrite applies cleanly now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleasedReport {
    pub branch: String,
    pub verb: String,
}

/// A hold dropped on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AbandonedHold {
    pub branch: String,
    pub verb: String,
    /// Set when a resolution session was open and its markers were thrown away.
    pub was_resolving: bool,
}

/// The result of `ff done`: an editing session ended, landed or abandoned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneOutcome {
    /// The session landed: the edited commit was amended and what waited
    /// ahead was replayed onto it.
    Done(DoneReport),
    /// The replay conflicts, so nothing was written and the rewrite is
    /// waiting. A hold is an outcome and not an error — something happened,
    /// and it has a report — even though the caller still exits 3, because a
    /// human decision is required before anything moves.
    Held(HeldReport),
    /// The session was dropped without landing.
    Abandoned(AbandonReport),
    /// A resolution session ended: the reader's fixes landed, each in the
    /// step that owned it, and the whole stack moved one time.
    Resolved(ResolvedReport),
}

/// A resolution that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedReport {
    pub branch: String,
    /// The verb whose rewrite this was.
    pub verb: String,
    /// Conflict regions the reader fixed.
    pub fixed: usize,
    /// Commits rewritten.
    pub replayed: usize,
    pub new_tip: String,
    /// Set when the landing left more rewrite behind and a fresh hold was
    /// recorded. A backstop rather than a common path: a landing changes
    /// every input to the question a hold asks, so asking again almost always
    /// comes back clean or expired. It is asked anyway, because the cost is
    /// one simulated replay and the alternative is a rewrite going quiet.
    pub still_held: Option<HeldReport>,
}

/// A session that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoneReport {
    /// The session branch that ended, and is now gone.
    pub session: String,
    /// The commit that was being edited, full sha.
    pub editing: String,
    /// What it became — equal to `editing` when the session changed nothing.
    /// `None` when the rewrite dropped the commit — it introduces nothing
    /// now, and fufu writes no empty commit — in which case it is named in
    /// `dropped`.
    pub amended: Option<String>,
    pub subject: String,
    /// The branch landed back on.
    pub onto: String,
    /// Commits replayed ahead of the amended one.
    pub replayed: usize,
    /// Other local branches the rewrite carried (not the session, not `onto`).
    pub moved: Vec<String>,
    /// `onto`'s new tip, full sha.
    pub new_tip: String,
    /// The session made no content change; nothing was rewritten.
    pub unchanged: bool,
    /// How many rewritten commits the branch's remote already has.
    pub published: usize,
    /// The tracking ref `published` was measured against, e.g. `origin/main`.
    pub published_on: Option<String>,
    /// What became of the change `ff edit` parked on `onto`.
    pub arrival: ArrivalReport,
    /// Worktree files written or deleted landing back.
    pub files: usize,
    /// Commits the rewrite dropped because they introduce nothing — fufu
    /// writes no empty commit. Oldest-first.
    pub dropped: Vec<crate::rewrite::Dropped>,
}

/// A session that was dropped without landing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AbandonReport {
    pub session: String,
    pub editing: String,
    pub subject: String,
    pub onto: String,
    /// The stash sha the session's uncommitted edits went to, if any.
    pub stashed: Option<String>,
    pub arrival: ArrivalReport,
    pub files: usize,
}

/// The result of a rewrite that conflicted: nothing was written, the intent
/// is waiting, and every status render says so until it is gone. A hold is an
/// outcome and not an error — something happened, and it has a report — even
/// though the process still exits 3, because a human decision is required
/// before anything moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeldReport {
    /// The verb that held: `restack`, `done`, `absorb`, `lift`.
    pub verb: String,
    /// The branch the hold stands on.
    pub branch: String,
    /// Where the replay stopped: a commit it could not reapply, or the open
    /// change, which cannot come along. Carried as `futures::At` rather than
    /// flattened to a sha and a subject, because a report that flattened it
    /// would have to name the open change by the commit it is sitting on,
    /// which is not where anything went wrong.
    pub at: crate::futures::At,
    /// The paths that stopped it.
    pub paths: Vec<String>,
    /// How many commits the rewrite would have replayed in all, so the report
    /// can say "1 of 5" rather than leaving the size of the stack unsaid.
    pub of: usize,
}

/// The result of `ff lift`: paths taken out of a commit and back into the
/// open change, and the restack it forced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiftOutcome {
    /// The paths were lifted out of the target and restacked.
    Lifted(LiftReport),
    /// The replay conflicts, so nothing was written and the rewrite is
    /// waiting. A hold is an outcome and not an error — something happened,
    /// and it has a report — even though the caller still exits 3, because a
    /// human decision is required before anything moves.
    Held(HeldReport),
    /// The selected paths are not among the ones that commit introduced.
    NothingToLift { from: String },
}

/// A lift that landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiftReport {
    /// The branch the lift ran on.
    pub branch: String,
    /// The target commit before the lift.
    pub from: String,
    /// The target commit after it. `None` when the rewrite dropped the
    /// commit — it introduces nothing now, and fufu writes no empty commit —
    /// in which case it is named in `dropped`.
    pub new: Option<String>,
    /// The target's subject, which a lift never changes.
    pub subject: String,
    /// Descendants restacked behind the target.
    pub restacked: usize,
    /// Other local branches carried with the rewrite, short names, sorted.
    pub moved: Vec<String>,
    /// How many of the rewritten commits the branch's remote already has.
    pub published: usize,
    /// The paths the filter selected; empty means the whole commit.
    pub paths: Vec<String>,
    /// Commits the rewrite dropped because they introduce nothing — fufu
    /// writes no empty commit. Oldest-first.
    pub dropped: Vec<crate::rewrite::Dropped>,
}

/// The result of `ff start` — always mints a fresh branch and parks
/// whatever was open where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartReport {
    /// The branch that was minted. start always mints.
    pub minted: String,
    /// Short name of what it forked from, for the "(forked from X)" line.
    pub forked_from: String,
    /// The stash sha the open change parked under, when the tree was dirty.
    pub parked: Option<String>,
}

/// One branch row for `ff branch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
    pub anonymous: bool,
    /// Tip sha; `None` for an unborn current branch.
    pub tip: Option<String>,
    pub subject: Option<String>,
    /// A parked change is waiting on this branch.
    pub parked: bool,
    pub pending_description: Option<String>,
    /// The branch this session will replay onto when it ends. `None` unless
    /// this branch is an unfinished editing session — and an unfinished one
    /// is worth noticing wherever branches are listed.
    pub session: Option<String>,
    /// A rewrite is held on this branch, waiting for `ff resolve`.
    pub held: bool,
    /// A resolution is open on this branch: its conflicts are in the working
    /// tree right now.
    pub resolving: bool,
    pub upstream: Option<Upstream>,
    /// What rebasing this branch onto its base would do.
    pub future: Option<crate::futures::Future>,
}

/// `ff branch` listing: named branches and anonymous ones, segregated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchList {
    pub named: Vec<BranchInfo>,
    pub anonymous: Vec<BranchInfo>,
}

/// The result of claiming an anonymous branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimReport {
    pub from: String,
    pub to: String,
    pub pre_op: Option<String>,
}

/// The result of deleting a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchDeleteReport {
    pub name: String,
    pub tip: String,
    /// Where the branch's pointer into the log was parked.
    pub trash_ref: Option<String>,
    /// The parked stash entry left behind in the stash stack, if any.
    pub parked_demoted: Option<String>,
    pub pre_op: Option<String>,
}

/// How an arrival (resuming a parked change) went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ArrivalReport {
    /// Nothing was parked on the target.
    None,
    /// The parked change is back in the working tree.
    Restored { stash: String, files: Vec<String> },
    /// The parked change no longer applies cleanly: it stays parked.
    StillParked { stash: String, paths: Vec<String> },
    /// The stash entry vanished outside fufu; the parked ref was demoted.
    Invalidated { stash: String },
}

/// The result of `ff switch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwitchReport {
    pub from: String,
    pub to: String,
    /// The stash sha the open change was parked under, if the tree was dirty.
    pub parked: Option<String>,
    pub arrival: ArrivalReport,
    /// The mandatory pre-verb capture, spelled as an operation id.
    pub pre_op: Option<String>,
}

/// The result of `ff commit` — the close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CommitOutcome {
    /// The open change was closed into a commit.
    Closed {
        id: String,
        short_id: String,
        /// The branch the close landed on (after any `-b` claim/fork).
        branch: String,
        subject: String,
        files_changed: usize,
        /// The anonymous branch this close claimed, if any.
        claimed_from: Option<String>,
        /// The mandatory pre-verb capture, spelled as an operation id.
        pre_op: Option<String>,
    },
}

/// One ref that moved outside fufu, absorbed by reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForeignChange {
    /// Full ref name, or `HEAD`.
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
    /// git's own reflog message for the move, when one exists.
    pub hint: Option<String>,
}

/// What one reconciliation pass found and did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    /// The operation log did not exist and was initialized.
    pub bootstrapped: bool,
    /// The log tip was unreadable; the log was parked and re-initialized.
    pub reinitialized: bool,
    /// Foreign motion absorbed by this pass (empty = clean).
    pub foreign: Vec<ForeignChange>,
    /// The operation this pass appended, in the letters alphabet, if any.
    pub entry: Option<String>,
    pub warnings: Vec<String>,
}

impl ReconcileReport {
    /// Nothing to report: no writes, no foreign motion, no warnings.
    pub fn is_quiet(&self) -> bool {
        !self.bootstrapped
            && !self.reinitialized
            && self.foreign.is_empty()
            && self.warnings.is_empty()
    }
}

/// One operation, for the operation-log view (`ff op log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpEntry {
    /// The letters spelling — an operation is addressed in letters, never hex.
    pub id: String,
    pub short_id: String,
    /// `op`, `foreign`, or `note`.
    pub kind: String,
    pub verb: String,
    pub summary: String,
    pub time: i64,
    pub branch: Option<String>,
    /// The tag the operation wears, if any. A session is a tag and nothing
    /// more, so it rides the row rather than grouping it.
    pub session: Option<String>,
    /// The operation this one undid, when the verb is `undo`.
    pub undo_of: Option<String>,
}

/// What `ff sync` did, one part per axis plus the exit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub branch: String,
    /// A fetch ran this invocation. `false` for `--no-fetch`, and for a
    /// repository with no remote to fetch from.
    pub fetched: bool,
    pub remote: RemoteAxis,
    pub base: BaseAxis,
    /// What `ff publish` would still have to do. Sync does not do it — but
    /// a branch that just lined up and still has something waiting is
    /// exactly when naming the other half is useful.
    pub pending: Pending,
}

/// What the outgoing half has left, as sync sees it. Three states rather
/// than a count, because "never published" is not zero commits waiting: it
/// is the case with the most to send and no shared copy to measure against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Pending {
    /// Nowhere to send it: the repository has no remote.
    NoRemote,
    /// A remote, but no shared copy of this branch yet. Publishing creates it.
    Unpublished,
    /// Commits the shared copy does not have. Zero is a real answer.
    Ahead(usize),
}

/// What `ff publish` reports: one branch, one exit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishReport {
    pub branch: String,
    pub publish: Publish,
    /// True when nothing was written and nothing was sent (dry run).
    pub dry_run: bool,
}

/// The remote axis: the shared copy of this same branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RemoteAxis {
    /// No upstream is configured, so there is nothing to reconcile with and
    /// publishing is what creates one.
    NoRemote,
    /// A tracking ref that is configured and absent: the shared copy was
    /// deleted. Sync says so and touches nothing.
    Gone { name: String },
    /// The branch and its remote diverged, this run's fetch brought nothing,
    /// and the operation log accounts for every commit the remote still
    /// holds: they are rewrites of your own, so the axis is outgoing and the
    /// publish is what handles it.
    Yours {
        name: String,
        ahead: usize,
        behind: usize,
    },
    /// The axis acted, and this is what `restack` made of it.
    Ran {
        name: String,
        outcome: RestackOutcome,
    },
}

/// The base axis: what this work sits on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BaseAxis {
    /// Nothing beneath this branch to answer to — standing on trunk, an
    /// editing session, or a trunk fufu cannot name.
    NoBase,
    /// The remote axis held, and the first axis that conflicts stops the run.
    Skipped,
    Ran {
        name: String,
        outcome: RestackOutcome,
    },
}

/// What `ff publish` did with the branch, or why it did nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Publish {
    /// There is no remote at all, so there is nowhere to send anything.
    NoRemote,
    /// A rewrite is held on this branch. This is the exits-blocked
    /// discipline, and the one thing publish refuses rather than reports.
    Blocked,
    /// The remote already holds everything this branch does.
    UpToDate,
    /// No upstream: the push creates the remote branch and starts tracking it.
    Create {
        remote: String,
        remote_branch: String,
        tip: String,
    },
    /// Send it, under a lease whose expected value is the tracking ref as it
    /// stands — "what I last saw", which is precisely what publish knows
    /// without going to the network itself. An empty lease is git's own
    /// spelling for *must not exist*, and re-creates a shared copy somebody
    /// deleted.
    Push {
        remote: String,
        remote_branch: String,
        lease: String,
        tip: String,
    },
}
