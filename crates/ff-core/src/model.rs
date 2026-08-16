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
    /// The snapshot the worktree was restored to.
    pub target: SnapEntry,
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
pub struct TrimChain {
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
    pub chains: Vec<TrimChain>,
    /// The one log's retention; `None` when no log exists yet.
    pub log: Option<TrimLog>,
    /// True when nothing was written (dry run).
    pub dry_run: bool,
}

/// The result of `ff undo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UndoReport {
    /// The operation that was undone, in the letters alphabet.
    pub target: String,
    pub target_summary: String,
    /// `op` or `foreign` — foreign undos are labeled.
    pub target_kind: String,
    /// How many operations rolled back (later ops roll back too).
    pub rolled_back: usize,
    /// Every ref moved, with old and new values.
    pub refs: Vec<crate::ops::RefTransition>,
    /// Where HEAD went, when it moved.
    pub head_moved: Option<String>,
    /// Worktree files written or deleted.
    pub files: Vec<String>,
    pub warnings: Vec<String>,
    pub pre_op: Option<String>,
}

/// The result of `ff describe` (pending-description edit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DescribeReport {
    pub branch: String,
    pub old: Option<String>,
    pub new: Option<String>,
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
    pub upstream: Option<Upstream>,
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
    /// Clean tree: nothing to close, nothing was written.
    NothingToClose { branch: String },
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

/// One non-capture operation, for the ops view (`ff log --ops`).
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
    /// The operation this one undid, when the verb is `undo`.
    pub undo_of: Option<String>,
}
