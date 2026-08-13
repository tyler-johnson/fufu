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
