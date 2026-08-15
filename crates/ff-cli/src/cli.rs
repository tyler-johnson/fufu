use std::ffi::OsString;

use clap::{Parser, Subcommand};

use crate::help;

/// Bare `ff` is the snapshot verb (jj-style): `ff [-m <msg>]` takes a manual
/// snapshot; every other command captures first, then does its work.
/// `args_conflicts_with_subcommands` makes `ff -m x status` a usage error.
#[derive(Parser)]
#[command(
    name = "ff",
    // Pinned, not derived: clap falls back to argv[0]'s file name, which is
    // "ff.exe" on Windows, so usage lines would read "Usage: ff.exe hook agent"
    // there and "Usage: ff hook agent" everywhere else. Subcommands inherit it.
    bin_name = "ff",
    version = concat!(env!("CARGO_PKG_VERSION"), env!("FF_BUILD_INFO")),
    about = "a friendlier interface to plain git",
    long_about = help::ROOT,
    after_long_help = help::ROOT_EXAMPLES,
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
    #[command(long_about = help::STATUS, after_long_help = help::STATUS_EXAMPLES)]
    Status {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the timeline: snapshots interleaved with commits
    #[command(long_about = help::LOG, after_long_help = help::LOG_EXAMPLES)]
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
    #[command(long_about = help::EVOLOG, after_long_help = help::EVOLOG_EXAMPLES)]
    Evolog {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
    },
    /// Capture-first git passthrough; daily forms translate to ff verbs
    #[command(
        disable_help_flag = true,
        long_about = help::GIT,
        after_long_help = help::GIT_EXAMPLES
    )]
    Git {
        /// Arguments passed to git verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Restore worktree files from the timeline
    #[command(long_about = help::RESTORE, after_long_help = help::RESTORE_EXAMPLES)]
    Restore {
        /// Snapshot to restore from: an id, '@{1}', 30m/2h/1d/1w, or a date
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
    /// Drop snapshots past the retention cutoff (fufu.keep, 90d)
    #[command(long_about = help::TRIM, after_long_help = help::TRIM_EXAMPLES)]
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
    #[command(long_about = help::COMMIT, after_long_help = help::COMMIT_EXAMPLES)]
    Commit {
        /// Describe what is closing; wins over the pending description
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
        /// Branch to land the close on: claim an anonymous one, or fork here
        #[arg(short = 'b', value_name = "branch")]
        branch: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Switch branches; a dirty tree is parked, a parked change resumes
    #[command(long_about = help::SWITCH, after_long_help = help::SWITCH_EXAMPLES)]
    Switch {
        /// Branch name, or a unique prefix of one
        #[arg(value_name = "branch")]
        target: String,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Roll the repository back to the state before an operation
    #[command(long_about = help::UNDO, after_long_help = help::UNDO_EXAMPLES)]
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
    /// Begin new work on a fresh branch
    #[command(
        visible_alias = "new",
        long_about = help::START,
        after_long_help = help::START_EXAMPLES
    )]
    Start {
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
    #[command(long_about = help::DESCRIBE, after_long_help = help::DESCRIBE_EXAMPLES)]
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
    #[command(long_about = help::BRANCH, after_long_help = help::BRANCH_EXAMPLES)]
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
    #[command(long_about = help::HOOK, after_long_help = help::HOOK_EXAMPLES)]
    Hook {
        #[command(subcommand)]
        kind: HookKind,
    },
    /// Read and write fufu's settings (plain git config under fufu.*)
    #[command(long_about = help::CONFIG, after_long_help = help::CONFIG_EXAMPLES)]
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
    /// Verify the safety net: chains, identity, reflogs, gc guard, wiring
    #[command(long_about = help::DOCTOR, after_long_help = help::DOCTOR_EXAMPLES)]
    Doctor {
        /// Repair the gc config keys (the one write doctor performs)
        #[arg(long)]
        fix: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Download the latest release and replace this binary
    #[command(long_about = help::UPDATE, after_long_help = help::UPDATE_EXAMPLES)]
    Update {
        /// Refresh the update cache only (used by the background check)
        #[arg(long)]
        check: bool,
    },
}

/// Everything that feeds the capture floor is a hook. One grammar:
/// `ff hook <agent|shell|editor> <install|uninstall|list|trigger> [name]`.
#[derive(Subcommand)]
pub enum HookKind {
    /// Agent hooks (claude): capture around agent tool actions
    #[command(long_about = help::HOOK_AGENT, after_long_help = help::HOOK_AGENT_EXAMPLES)]
    Agent {
        #[command(subcommand)]
        verb: HookVerb,
    },
    /// Shell hooks (bash, zsh, fish): the `alias git='ff git'` line
    #[command(long_about = help::HOOK_SHELL, after_long_help = help::HOOK_SHELL_EXAMPLES)]
    Shell {
        #[command(subcommand)]
        verb: HookVerb,
    },
    /// Editor hooks: reserved — none exist yet
    #[command(long_about = help::HOOK_EDITOR)]
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
    /// Hook runtime, called by the client with a payload on stdin
    #[command(long_about = help::HOOK_TRIGGER)]
    Trigger {
        /// Agent name; defaults to claude
        name: Option<String>,
    },
}
