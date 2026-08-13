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
        /// Snapshot to restore from: id, @{n}, 30m/2h/1d/1w, or a date;
        /// defaults to the newest snapshot
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
    /// Manage fufu's capture hooks: agents, shells, editors
    Hook {
        #[command(subcommand)]
        kind: HookKind,
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
