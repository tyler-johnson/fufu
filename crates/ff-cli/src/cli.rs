use std::ffi::OsString;

use clap::{Parser, Subcommand};

/// Bare `ff` is the snapshot verb (jj-style): `ff [-m <msg>]` takes a manual
/// snapshot; every other command captures first, then does its work.
/// `args_conflicts_with_subcommands` makes `ff -m x status` a usage error.
#[derive(Parser)]
#[command(
    name = "ff",
    version,
    about = "a friendlier interface to plain git",
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Message for the manual snapshot
    #[arg(short = 'm', value_name = "msg")]
    pub message: Option<String>,
    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show the working tree status
    Status {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the timeline: snapshots interleaved with commits
    Log {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
        /// Commits only — the plain history view
        #[arg(long)]
        commits: bool,
        /// The operation journal — every fufu mutation, with op ids
        #[arg(long, conflicts_with = "commits")]
        ops: bool,
    },
    /// Show the open change's snapshot chain (the evolution log)
    Evolog {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
    },
    /// Capture-first git passthrough; daily forms translate to ff verbs
    #[command(disable_help_flag = true)]
    Git {
        /// Arguments passed to git verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Restore worktree files from the timeline
    Restore {
        /// Snapshot to restore from: id (hex or letters as shown by ff
        /// evolog), @{n}, 30m/2h/1d/1w, or a date; defaults to the newest
        /// snapshot
        #[arg(long, value_name = "target")]
        at: Option<String>,
        /// Restore the entire worktree to the target state
        #[arg(long, conflicts_with = "paths")]
        all: bool,
        /// Paths to restore from the target
        #[arg(value_name = "path", required_unless_present = "all")]
        paths: Vec<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Drop snapshots older than the retention cutoff (fufu.keep, 90d default)
    Trim {
        /// Report what would be dropped without writing anything
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Also drop whole chains whose branch no longer exists
        #[arg(long)]
        gone: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Close the open change into a commit (the working tree is the change)
    Commit {
        /// Describe what is closing; wins over the pending description
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
        /// Land the close on this branch: claims an anonymous branch, or
        /// forks a fresh one here (the old branch stays)
        #[arg(short = 'b', value_name = "branch")]
        branch: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch branches; a dirty tree is parked, a parked change resumes
    Switch {
        /// Branch name, or a unique prefix of one
        #[arg(value_name = "branch")]
        target: String,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Roll the repository back to the state before an operation
    Undo {
        /// Op id (journal-sha prefix, see ff log --ops); newest if omitted
        #[arg(value_name = "op")]
        op: Option<String>,
        /// Roll back what remains even if parts were trimmed
        #[arg(long)]
        force: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Open a new change: close the current one, optionally moving first
    New {
        /// Branch, revision, or nothing to stay here
        #[arg(value_name = "target")]
        target: Option<String>,
        /// Pending description for the change being opened
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Name for the minted/forked branch (or claim a placeholder)
        #[arg(short = 'b', value_name = "branch")]
        branch: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit the pending description of the open change
    Describe {
        /// The description text; omitted opens $EDITOR
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Rename the current branch instead (proper names allowed)
        #[arg(short = 'b', value_name = "branch", conflicts_with = "message")]
        branch: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List branches, claim the current anonymous branch, or delete one
    Branch {
        /// Claim the current anonymous branch with this name
        #[arg(value_name = "name")]
        name: Option<String>,
        /// Delete a branch (timeline moves to trash; undoable via ff undo)
        #[arg(
            short = 'd',
            long = "delete",
            value_name = "branch",
            conflicts_with = "name"
        )]
        delete: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage fufu's capture hooks: agents, shells, editors
    Hook {
        #[command(subcommand)]
        kind: HookKind,
    },
    /// Read and write fufu's settings (plain git config under fufu.*)
    Config {
        /// Setting name — case-insensitive, the fufu. prefix optional
        #[arg(value_name = "key")]
        key: Option<String>,
        /// New value to set for this repo (--global: every repo)
        #[arg(value_name = "value", conflicts_with = "unset")]
        value: Option<String>,
        /// Remove the setting, returning to the default
        #[arg(long, requires = "key")]
        unset: bool,
        /// Apply the set/unset to every repo (user-level git config)
        #[arg(long)]
        global: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

/// Everything that feeds the capture floor is a hook. One grammar:
/// `ff hook <agent|shell|editor> <install|uninstall|list|trigger> [name]`.
#[derive(Subcommand)]
pub enum HookKind {
    /// Agent hooks (claude): capture around agent tool actions
    Agent {
        #[command(subcommand)]
        verb: HookVerb,
    },
    /// Shell hooks (bash, zsh, fish): the `alias git='ff git'` line
    Shell {
        #[command(subcommand)]
        verb: HookVerb,
    },
    /// Editor hooks: reserved — none exist yet
    Editor {
        #[command(subcommand)]
        verb: HookVerb,
    },
    /// Unknown kinds exit 0 silently: never break a caller's hook.
    /// (Also forwards the committed Phase 1 spelling `ff hook claude`.)
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub enum HookVerb {
    /// Install the hook (agent: settings entries; shell: the alias line)
    Install {
        /// Agent (claude) or shell (bash, zsh, fish); defaults to claude / $SHELL
        name: Option<String>,
    },
    /// Remove exactly what install added
    Uninstall {
        /// Agent or shell name; defaults to claude / $SHELL
        name: Option<String>,
    },
    /// Show installation state
    List {
        /// Optional name to narrow to
        name: Option<String>,
    },
    /// Hook runtime, invoked by the client with a payload on stdin.
    /// Agent triggers always exit 0 — a hook must never veto an action.
    Trigger {
        /// Agent name; defaults to claude
        name: Option<String>,
    },
}
