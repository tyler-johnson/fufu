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

/// An operation in progress in the repository, mirroring `gix::state::InProgress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
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

impl From<gix::state::InProgress> for Operation {
    fn from(state: gix::state::InProgress) -> Self {
        use gix::state::InProgress as S;
        match state {
            S::ApplyMailbox => Operation::ApplyMailbox,
            S::ApplyMailboxRebase => Operation::ApplyMailboxRebase,
            S::Bisect => Operation::Bisect,
            S::CherryPick => Operation::CherryPick,
            S::CherryPickSequence => Operation::CherryPickSequence,
            S::Merge => Operation::Merge,
            S::Rebase => Operation::Rebase,
            S::RebaseInteractive => Operation::RebaseInteractive,
            S::Revert => Operation::Revert,
            S::RevertSequence => Operation::RevertSequence,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub head: HeadState,
    pub operation: Option<Operation>,
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

/// The result of one capture attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SnapOutcome {
    /// A new snapshot commit was written to the chain ref.
    Created {
        id: String,
        short_id: String,
        /// The chain ref the snapshot went to, e.g. `refs/fufu/snap/main`.
        r#ref: String,
        /// Files changed relative to the previous snapshot (or the base).
        changed_files: usize,
        /// Worktree files skipped for exceeding `fufu.maxFileSize`.
        skipped_files: Vec<String>,
        /// Non-fatal problems (e.g. the gc-config write failing).
        warnings: Vec<String>,
    },
    /// The tree is already captured: nothing to record.
    NoOp {
        r#ref: String,
        /// Current chain tip, or `None` when the chain doesn't exist yet.
        tip: Option<String>,
    },
    /// Another capture holds the ref lock or won the CAS race; this one skips.
    Contended { r#ref: String },
}

/// One snapshot commit on a capture chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapEntry {
    pub id: String,
    pub short_id: String,
    pub subject: String,
    /// Committer time, seconds since the unix epoch.
    pub time: i64,
    /// The HEAD commit the snapshot was taken on (base edge), if any.
    pub base: Option<String>,
    /// The previous snapshot on the chain, if any.
    pub prev: Option<String>,
}

/// One row of the interleaved timeline, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelineRow {
    Snapshot(SnapEntry),
    /// A base-edge change (HEAD moved between snapshots), or the final anchor
    /// commit the chain grew from.
    Base(LogEntry),
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
    /// The mandatory pre-restore snapshot, when one was created.
    pub pre_snapshot: Option<String>,
}

/// Per-chain result of a trim pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrimChain {
    /// The chain ref, e.g. `refs/fufu/snap/main`.
    pub r#ref: String,
    /// The chain's branch name (or `@detached`).
    pub branch: String,
    pub dropped: usize,
    pub kept: usize,
    /// Where the pre-trim tip was saved, when anything was dropped.
    pub trash_ref: Option<String>,
    /// True when the whole chain was dropped and the ref deleted.
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrimReport {
    pub chains: Vec<TrimChain>,
    /// True when nothing was written (dry run).
    pub dry_run: bool,
}
