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
    /// Manage the shell alias (`alias git='ff git'`)
    Shell {
        #[command(subcommand)]
        action: ShellAction,
    },
    /// Agent hook runtime and installers
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
pub enum ShellAction {
    /// Add the alias to the shell's rc file
    Install {
        /// Shell to install for (bash, zsh, fish); defaults to $SHELL
        shell: Option<String>,
    },
    /// Remove exactly the lines `ff shell install` added
    Uninstall {
        /// Shell to uninstall from; defaults to $SHELL
        shell: Option<String>,
    },
    /// Show alias status for each known shell
    List,
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Claude Code hook runtime: reads the hook payload on stdin.
    /// Always exits 0 — a hook must never veto an agent action.
    Claude,
    /// Install the hook entries into the client's settings
    Install {
        /// Hook client (claude)
        client: String,
    },
    /// Remove the hook entries from the client's settings
    Uninstall {
        /// Hook client (claude)
        client: String,
    },
    /// Show hook installation status
    List {
        /// Hook client (claude)
        client: String,
    },
    /// Unknown hook clients exit 0 silently: never break a caller's hook.
    #[command(external_subcommand)]
    Other(#[allow(dead_code)] Vec<OsString>),
}
