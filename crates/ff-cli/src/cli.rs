use std::ffi::OsString;

use clap::{Parser, Subcommand};

use crate::help;

/// The name the tool has, as opposed to the two letters it is typed as. The
/// version is the one place the full word is worth spending a line on: it is
/// what somebody searches for, and `ff` is not a searchable string.
pub const NAME: &str = "fufu";

/// What `ff -v` and `ff version` both print: the release, the commit it was
/// built from, and the project's home under it. Both spellings go through the
/// verb, which prepends the name by hand — clap no longer prints this. One
/// constant, so the spellings cannot answer the same question differently.
///
/// The URL comes from the manifest rather than from a literal here — there is
/// already one place that records where this lives, and a second would be a
/// place to forget.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    env!("FF_BUILD_INFO"),
    "\n",
    env!("CARGO_PKG_REPOSITORY")
);

/// Bare `ff` is the map (jj-style): the local branches as a skeleton —
/// tips, merges, forks — the answer to "where did I leave that idea?".
/// Capture is automatic and every verb takes it first, so `-m` is declared
/// only to stay hidden — typing it is answered in main rather than met with
/// clap's bare "unexpected argument", the same reason `--ops` is still
/// declared at log's `-r`.
#[derive(Parser)]
#[command(
    name = "ff",
    // Pinned, not derived: clap falls back to argv[0]'s file name, which is
    // "ff.exe" on Windows, so usage lines would read "Usage: ff.exe hook agent"
    // there and "Usage: ff hook agent" everywhere else. Subcommands inherit it.
    bin_name = "ff",
    version = VERSION,
    // The version line's name, and only that: usage lines keep `bin_name`,
    // which is what you actually type.
    display_name = NAME,
    // Declared by hand below, for the short letter: clap's own flag is `-V`.
    disable_version_flag = true,
    about = "a friendlier interface to plain git",
    long_about = help::ROOT,
    after_long_help = help::ROOT_EXAMPLES
)]
pub struct Cli {
    /// Retired: bare ff is the map, and capture is automatic
    #[arg(short = 'm', value_name = "msg", hide = true)]
    pub message: Option<String>,
    /// Branches to show, newest tip first; 0 means all
    #[arg(short = 'n', long = "max-count", value_name = "count")]
    pub branches: Option<usize>,
    /// Every local branch
    #[arg(long)]
    pub all: bool,
    /// Emit machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,
    // `-v`, not clap's default `-V`. fufu has no verbose flag to reserve the
    // lowercase letter for — verbosity here is `--json` or a different verb —
    // so the shifted spelling bought nothing and cost every person who typed
    // the lowercase one first. `-V` is gone rather than kept as an alias: a
    // second spelling for a one-line answer is surface with no reader.
    /// Print the version and the commit it was built from
    #[arg(short = 'v', long = "version", action = clap::ArgAction::SetTrue)]
    pub version: bool,
    /// Retired: the version flag is lowercase `-v`
    ///
    /// Declared only to stay hidden, on the same rule as `-m` and `--ops`:
    /// `-V` is what almost every other tool spells this, so typing it is a
    /// question rather than a typo, and clap's bare "unexpected argument"
    /// answers a different one.
    #[arg(short = 'V', hide = true, action = clap::ArgAction::SetTrue)]
    pub version_shouted: bool,
    /// Session name for this invocation
    #[arg(long, value_name = "name", global = true)]
    pub session: Option<String>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

// A `// agent notice quotes this` line marks surface that the once-per-session
// Claude briefing spells out verbatim (`NOTICE` in cmd/hook.rs). That briefing
// is the only spelling lesson an agent gets, so a retired verb or a renamed
// flag there teaches it to fail: change one here and fix it there in the same
// commit. `grep -rn "agent notice" crates/ff-cli/src` finds every site, both
// directions.
#[derive(Subcommand)]
pub enum Command {
    /// The map bare `ff` draws: the local branches as a skeleton
    #[command(long_about = help::ROOT, after_long_help = help::ROOT_EXAMPLES)]
    Map {
        /// Branches to show, newest tip first; 0 means all
        #[arg(short = 'n', long = "max-count", value_name = "count")]
        branches: Option<usize>,
        /// Every local branch
        #[arg(long)]
        all: bool,
    },
    /// Which branches in flight would collide with each other
    Collide {
        /// Branches to compare; omitted means the ones the map would rank
        names: Vec<String>,
        /// Branches to rank in, newest tip first; 0 means all
        #[arg(short = 'n', long = "max-count", value_name = "count")]
        branches: Option<usize>,
        /// Every local branch
        #[arg(long)]
        all: bool,
    },
    // agent notice quotes this: `ff status`
    /// Show the working tree status
    #[command(alias = "st", long_about = help::STATUS, after_long_help = help::STATUS_EXAMPLES)]
    Status {
        #[command(flatten)]
        past: Past,
    },
    // agent notice quotes this: `ff log`
    /// Show the timeline: commits wearing the operations that built them
    #[command(long_about = help::LOG, after_long_help = help::LOG_EXAMPLES)]
    Log {
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
        /// Revisions to show, as a revset; without it, the walk from HEAD
        #[arg(short = 'r', long = "revisions", value_name = "revset")]
        revisions: Option<String>,
        /// Commits only — the plain history view
        #[arg(long)]
        commits: bool,
        /// Retired: the operation log is `ff op log`
        #[arg(long, hide = true, conflicts_with = "commits")]
        ops: bool,
        /// Files or directories to limit the log to; all of them when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
        #[command(flatten)]
        past: Past,
    },
    // agent notice quotes this: `ff diff`
    /// Show the open change as a patch — content, not just counts
    #[command(long_about = help::DIFF, after_long_help = help::DIFF_EXAMPLES)]
    Diff {
        /// Files or directories to limit the patch to; all of them when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    /// Show one commit: what it was, and what it did
    #[command(long_about = help::SHOW, after_long_help = help::SHOW_EXAMPLES)]
    Show {
        /// The revision; `@`, the open change, when omitted
        #[arg(value_name = "rev")]
        rev: Option<String>,
        /// Files or directories to limit the patch to; all of them when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    // agent notice quotes this: `ff history`
    /// Where you can go back to: one row per `ff undo` step, with redo above
    #[command(long_about = help::HISTORY, after_long_help = help::HISTORY_EXAMPLES)]
    History {
        /// Number of undo steps to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
    },
    /// Show the open change's operations, newest first (the evolution log)
    #[command(alias = "ev", long_about = help::EVOLOG, after_long_help = help::EVOLOG_EXAMPLES)]
    Evolog {
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
        /// Print each row's patch under it — what that operation changed
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        #[command(flatten)]
        past: Past,
    },
    // agent notice quotes this: `ff git <args…>`
    /// Capture-first git passthrough; translation to ff verbs is opt-in
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
    // agent notice quotes this: `ff restore <path>`, `--all --at <time>`, `--at-op <id>`
    /// Restore worktree files from the timeline
    #[command(long_about = help::RESTORE, after_long_help = help::RESTORE_EXAMPLES)]
    Restore {
        /// Revision to restore from; without it, the commit under the change
        #[arg(long, value_name = "rev")]
        from: Option<String>,
        /// Restore the entire worktree to the source state
        #[arg(long, conflicts_with = "paths")]
        all: bool,
        /// Paths to restore from the source
        #[arg(value_name = "path", required_unless_present = "all")]
        paths: Vec<String>,
        #[command(flatten)]
        past: Past,
    },
    /// Drop operations past the retention cutoff (fufu.keep, 90d)
    #[command(long_about = help::TRIM, after_long_help = help::TRIM_EXAMPLES)]
    Trim {
        /// Report what would be dropped without writing anything
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Also drop the pointers of branches that no longer exist
        #[arg(long)]
        gone: bool,
    },
    // agent notice quotes this: `ff commit -m`
    /// Close the open change into a commit (the working tree is the change)
    #[command(alias = "ci", long_about = help::COMMIT, after_long_help = help::COMMIT_EXAMPLES)]
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
        /// Files or directories to close, leaving the rest open; all of it when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    // agent notice quotes this: `ff switch <branch>`
    /// Switch branches; a dirty tree is parked, a parked change resumes
    #[command(alias = "sw", long_about = help::SWITCH, after_long_help = help::SWITCH_EXAMPLES)]
    Switch {
        /// Branch name, or a unique prefix of one
        #[arg(value_name = "branch")]
        target: String,
    },
    // agent notice quotes this: `ff undo`
    /// Step the whole repository back one run of work
    #[command(long_about = help::UNDO, after_long_help = help::UNDO_EXAMPLES)]
    Undo,
    /// Step forward again after an undo
    #[command(long_about = help::REDO, after_long_help = help::REDO_EXAMPLES)]
    Redo,
    /// The operation log as objects: read it, compare it, move to it
    #[command(long_about = help::OP, after_long_help = help::OP_EXAMPLES)]
    Op {
        #[command(subcommand)]
        action: OpAction,
    },
    // agent notice quotes this: `ff start`
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
    },
    /// Edit the pending description of the open change
    #[command(alias = "desc", long_about = help::DESCRIBE, after_long_help = help::DESCRIBE_EXAMPLES)]
    Describe {
        /// The revision to reword; omitted describes the open change
        #[arg(value_name = "rev", conflicts_with = "branch")]
        rev: Option<String>,
        /// The description text; omitted opens $EDITOR
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Name the branch you are on instead — anonymous or already named
        #[arg(short = 'b', value_name = "branch", conflicts_with = "message")]
        branch: Option<String>,
    },
    // agent notice quotes this: `ff absorb --into <rev>`
    /// Fold working changes into a commit that has already closed
    #[command(long_about = help::ABSORB, after_long_help = help::ABSORB_EXAMPLES)]
    Absorb {
        /// Commit to absorb into; without it, the commit under the change
        #[arg(long, value_name = "rev")]
        into: Option<String>,
        /// Limit the absorb to these paths (files or directory prefixes)
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    /// Take changes back out of a closed commit, into the open change
    #[command(long_about = help::LIFT, after_long_help = help::LIFT_EXAMPLES)]
    Lift {
        /// Commit to lift out of; without it, the commit under the change
        #[arg(long, value_name = "rev")]
        from: Option<String>,
        /// Limit the lift to these paths (files or directory prefixes)
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    /// Replay a branch's commits onto the base it sits on
    #[command(long_about = help::RESTACK, after_long_help = help::RESTACK_EXAMPLES)]
    Restack {
        /// Branch to restack; without it, the one you are on
        #[arg(value_name = "branch")]
        branch: Option<String>,
        /// Base to replay onto; recorded as this branch's new parent
        #[arg(long, value_name = "branch")]
        onto: Option<String>,
    },
    // agent notice quotes this: `ff sync`
    /// Line this branch up with its base and its remote
    #[command(long_about = help::SYNC, after_long_help = help::SYNC_EXAMPLES)]
    Sync {
        /// Skip the fetch: reconcile with what you already have
        #[arg(long)]
        no_fetch: bool,
    },
    // agent notice quotes this: `ff publish`
    /// Send this branch to its remote, under a lease
    #[command(long_about = help::PUBLISH, after_long_help = help::PUBLISH_EXAMPLES)]
    Publish {
        /// Say which push this would be, without sending it
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Send to this remote, and record that the branch answers to it
        #[arg(long, value_name = "remote")]
        to: Option<String>,
    },
    /// What the remotes here are called, and where each one points
    #[command(long_about = help::REMOTE, after_long_help = help::REMOTE_EXAMPLES)]
    Remote,
    /// Start a repository with the safety net already on
    #[command(long_about = help::INIT, after_long_help = help::INIT_EXAMPLES)]
    Init {
        /// Where to create it; the current directory when omitted
        #[arg(value_name = "dir")]
        dir: Option<String>,
        /// Refused: a bare repository has no working tree to capture
        #[arg(long, hide = true)]
        bare: bool,
    },
    /// Clone a repository, and arm it on arrival
    #[command(long_about = help::CLONE, after_long_help = help::CLONE_EXAMPLES)]
    Clone {
        /// The repository to clone from
        #[arg(value_name = "url")]
        url: String,
        /// Where to put it; the URL's last path segment when omitted
        #[arg(value_name = "dir")]
        dir: Option<String>,
        /// Check out this branch instead of the remote's HEAD
        #[arg(short = 'b', long, value_name = "name")]
        branch: Option<String>,
        /// Shallow: only the last <n> commits
        #[arg(long, value_name = "n")]
        depth: Option<std::num::NonZeroU32>,
        /// Name for the remote
        #[arg(short = 'o', long, value_name = "name", default_value = "origin")]
        origin: String,
    },
    /// Open an editing session on a commit: go there, edit it, come back
    #[command(long_about = help::EDIT, after_long_help = help::EDIT_EXAMPLES)]
    Edit {
        /// The commit to edit. A branch name is a switch instead
        #[arg(value_name = "rev")]
        rev: String,
    },
    /// Finish the editing session: amend, replay what waited, land back
    #[command(long_about = help::DONE, after_long_help = help::DONE_EXAMPLES)]
    Done {
        /// Drop the session instead of landing it
        #[arg(long)]
        abandon: bool,
    },
    /// Materialize a held rewrite's conflicts and fix them, all at once
    #[command(long_about = help::RESOLVE, after_long_help = help::RESOLVE_EXAMPLES)]
    Resolve {
        /// Drop the pending rewrite instead of resolving it
        #[arg(long)]
        abandon: bool,
    },
    /// Manage lines of work: what exists, and removing one
    #[command(alias = "br", long_about = help::BRANCH, after_long_help = help::BRANCH_EXAMPLES)]
    Branch {
        #[command(subcommand)]
        action: Option<BranchAction>,
        #[command(flatten)]
        past: Past,
    },
    /// Manage fufu's capture hooks: agents, shells, editors
    #[command(long_about = help::HOOK, after_long_help = help::HOOK_EXAMPLES)]
    Hook {
        #[command(subcommand)]
        kind: HookKind,
    },
    /// Read and write fufu's settings (plain git config under fufu.*)
    #[command(alias = "cfg", long_about = help::CONFIG, after_long_help = help::CONFIG_EXAMPLES)]
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
    },
    /// Verify the safety net: the log, identity, reflogs, gc guard, wiring
    #[command(long_about = help::DOCTOR, after_long_help = help::DOCTOR_EXAMPLES)]
    Doctor {
        /// Repair the gc config keys (the one write doctor performs)
        #[arg(long)]
        fix: bool,
    },
    /// Look up an error id and see what it means
    Explain {
        /// The error id to look up
        #[arg(value_name = "id")]
        id: Option<String>,
        /// List every error id fufu knows
        #[arg(long)]
        list: bool,
    },
    /// Which fufu this is, and whether it is the current one
    #[command(long_about = help::VERSION, after_long_help = help::VERSION_EXAMPLES)]
    Version,
    /// Download the latest release and replace this binary
    #[command(long_about = help::UPDATE, after_long_help = help::UPDATE_EXAMPLES)]
    Update {
        /// Refresh the update cache only (used by the background check)
        #[arg(long)]
        check: bool,
    },

    // The foreign verbs: git words fufu answers rather than runs. They are
    // declared for the same reason the retired `-m` and `--ops` are — a word
    // fufu deliberately does not have is a question, and clap's bare
    // "unrecognized subcommand" answers a question it never asked. Hidden,
    // because the command list is what fufu *does*; each one carries its own
    // arguments so `ff checkout main` reaches the answer instead of dying on
    // an unexpected argument first.
    /// git's checkout, split in two: `ff switch` moves, `ff restore` brings files back
    #[command(hide = true, alias = "co")]
    Checkout {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff stash`: switching parks the open change and resumes what waits
    #[command(hide = true)]
    Stash {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff pull`: `ff sync` takes in, `ff git pull` still runs git's
    #[command(hide = true)]
    Pull {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff push`: `ff publish` sends this branch, under a lease
    #[command(hide = true)]
    Push {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff rebase` yet: `ff status` costs it, `ff git rebase` runs it capture-first
    #[command(hide = true)]
    Rebase {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff merge`: fufu replays — `ff restack --onto`, `ff sync` — rather than merging
    #[command(hide = true)]
    Merge {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff blame`: history reads stay git's; `ff evolog` is the part blame cannot see
    #[command(hide = true)]
    Blame {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff tag`: `ff git tag` makes one, and `ff undo` is what puts a lost one back
    #[command(hide = true)]
    Tag {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
}

/// The two context flags, declared per-verb rather than `global = true`.
///
/// DESIGN scopes them to the verbs that *read*: `--at-op` and `--at` place a
/// command against a past state, and a verb that only adds to now has no
/// input state to place. So `ff commit --at 2h` is an unknown flag here
/// rather than an accepted-then-refused one — the parser carries the rule,
/// which is also what lets a verb claim any letter without consulting a list.
///
/// Two flags rather than one is what holds each to a single kind: an id is
/// never a date, and a date is never an id.
// agent notice quotes this: `ff restore --all --at <time>`, `--at-op <id>`
#[derive(clap::Args, Debug, Default)]
pub struct Past {
    /// Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)
    #[arg(long = "at-op", value_name = "op")]
    pub at_op: Option<String>,
    /// Read as of the operation current at this time (30m/2h/3d, or a date)
    #[arg(long = "at", value_name = "time", conflicts_with = "at_op")]
    pub at: Option<String>,
}

/// `ff op` — the operation log as objects. The envelope names the full path
/// (`"op log"`, not `"op"`), so two shapes never share one name.
#[derive(Subcommand)]
pub enum OpAction {
    // agent notice quotes this: `ff op log`
    /// Every operation, newest first, with the ids these verbs take
    #[command(long_about = help::OP_LOG, after_long_help = help::OP_LOG_EXAMPLES)]
    Log {
        /// Operations to show, as a revset over the operation log
        #[arg(value_name = "revset")]
        revset: Option<String>,
        /// Number of rows to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
        /// Retired: the expression is this verb's argument now
        #[arg(short = 'r', long = "revisions", value_name = "revset", hide = true)]
        revisions: Option<String>,
        /// Retired: every operation is shown, and `ff history` is the view
        #[arg(long, hide = true)]
        captures: bool,
        #[command(flatten)]
        past: Past,
    },
    /// Show one operation: what it was, what it moved, what it holds
    #[command(long_about = help::OP_SHOW, after_long_help = help::OP_SHOW_EXAMPLES)]
    Show {
        /// The operation; `@` (the newest) when omitted
        #[arg(value_name = "op")]
        op: Option<String>,
        /// Print the patch under the diffstat, not just the counts
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        #[command(flatten)]
        past: Past,
    },
    /// Compare the worktrees two operations carry
    #[command(long_about = help::OP_DIFF, after_long_help = help::OP_DIFF_EXAMPLES)]
    Diff {
        /// The older operation
        #[arg(value_name = "a")]
        a: String,
        /// The newer operation; `@` when omitted
        #[arg(value_name = "b")]
        b: Option<String>,
        /// Print the patch under the diffstat, not just the counts
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        #[command(flatten)]
        past: Past,
    },
    /// Rewind the whole repository to an operation
    #[command(long_about = help::OP_RESTORE, after_long_help = help::OP_RESTORE_EXAMPLES)]
    Restore {
        /// The operation to land on
        #[arg(value_name = "op")]
        op: String,
        /// Rewind to what remains even if parts were trimmed
        #[arg(long)]
        force: bool,
    },
    /// Invert one operation, leaving later work standing
    #[command(long_about = help::OP_REVERT, after_long_help = help::OP_REVERT_EXAMPLES)]
    Revert {
        /// The operation to invert
        #[arg(value_name = "op")]
        op: String,
    },
}

impl OpAction {
    /// The envelope name — the full path, never the bare family.
    fn name(&self) -> &'static str {
        match self {
            OpAction::Log { .. } => "op log",
            OpAction::Show { .. } => "op show",
            OpAction::Diff { .. } => "op diff",
            OpAction::Restore { .. } => "op restore",
            OpAction::Revert { .. } => "op revert",
        }
    }

    fn past(&self) -> Option<&Past> {
        match self {
            OpAction::Log { past, .. }
            | OpAction::Show { past, .. }
            | OpAction::Diff { past, .. } => Some(past),
            OpAction::Restore { .. } | OpAction::Revert { .. } => None,
        }
    }

    /// The family holds both readers and mutators, so the action decides:
    /// the reads capture at the CLI, the writes let the core's mandatory
    /// capture stand alone.
    fn lanes(&self) -> Lanes {
        match self {
            OpAction::Log { .. } | OpAction::Show { .. } | OpAction::Diff { .. } => Lanes::READ,
            OpAction::Restore { .. } | OpAction::Revert { .. } => Lanes::MUTATOR,
        }
    }
}

/// `ff branch` — the branch family, and it does not name anything. Naming
/// the branch you are on is `ff describe -b`, on the same axis as `-m`: one
/// verb for saying what a piece of work is, whether the subject is the
/// change's description or the branch's name. So this family is the
/// bookkeeping that is left — what exists, and taking one away.
#[derive(Subcommand)]
pub enum BranchAction {
    /// Named branches and anonymous ones, kept apart
    #[command(long_about = help::BRANCH_LIST, after_long_help = help::BRANCH_LIST_EXAMPLES)]
    List {
        /// Every remote-only branch, not just the newest few
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        past: Past,
    },
    /// Delete a branch — its timeline moves to trash, and `ff undo` is enough
    #[command(long_about = help::BRANCH_DELETE, after_long_help = help::BRANCH_DELETE_EXAMPLES)]
    Delete {
        /// The branch to delete, by its full name
        #[arg(value_name = "branch")]
        target: String,
        /// Remove the copy on the remote too — that half `ff undo` cannot reach
        #[arg(long)]
        shared: bool,
    },
    /// Anything else — most often the retired `ff branch <name>` claim.
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

impl BranchAction {
    /// The envelope name — the full path, as in the `ff op` family.
    fn name(&self) -> &'static str {
        match self {
            BranchAction::List { .. } | BranchAction::Other(_) => "branch list",
            BranchAction::Delete { .. } => "branch delete",
        }
    }

    fn past(&self) -> Option<&Past> {
        match self {
            BranchAction::List { past, .. } => Some(past),
            BranchAction::Delete { .. } | BranchAction::Other(_) => None,
        }
    }

    fn lanes(&self) -> Lanes {
        match self {
            // Bare `ff branch` is the list, and `Other` is a refusal that
            // renders like one: both read.
            BranchAction::List { .. } | BranchAction::Other(_) => Lanes::READ,
            // `ff_core::branch::delete` runs `begin_verb`, whose capture is
            // the mandatory pre-verb one — a second CLI capture would be a
            // full extra worktree diff.
            BranchAction::Delete { .. } => Lanes::MUTATOR,
        }
    }
}

/// The ambient lanes: what rides an invocation besides the verb itself —
/// the pre-command capture, the passive update lane (cache refresh,
/// auto-install, the one-line notice), and the daily auto-trim. One table on
/// `Command` decides them all, so a verb is in it by construction rather than
/// by somebody remembering to call three functions at the bottom of its
/// `run`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lanes {
    /// Take the pre-command snapshot at the CLI.
    pub capture: bool,
    /// Refresh the update cache in the background, and let an auto-install fire.
    pub update: bool,
    /// Print the "vX.Y.Z available" line on stderr when one is pending.
    pub notice: bool,
    /// Let the daily auto-trim ride this invocation.
    pub trim: bool,
}

impl Lanes {
    /// Nothing at all: the verbs that run before there is a repository, the
    /// detached `update` child, `git` and `hook` (each already carries its
    /// own lanes on purpose), and the foreign redirects (they print and stop).
    pub const NONE: Lanes = Lanes {
        capture: false,
        update: false,
        notice: false,
        trim: false,
    };

    /// A reader: the snapshot is the only pre-work it owes.
    pub const READ: Lanes = Lanes {
        capture: true,
        update: true,
        notice: true,
        trim: true,
    };

    /// A mutator: the core already captures, so a second one at the CLI
    /// would be a full extra worktree diff on the most expensive commands.
    pub const MUTATOR: Lanes = Lanes {
        capture: false,
        update: true,
        notice: true,
        trim: true,
    };

    /// `version`: the update lane with its voice removed — it prints its own
    /// "available" line, and it has no repository in its answer.
    pub const QUIET_UPDATE: Lanes = Lanes {
        capture: false,
        update: true,
        notice: false,
        trim: false,
    };
}

impl Command {
    /// The name this verb stamps on its JSON envelope, success or error. It
    /// lives beside the variant so a verb added without one is a compile
    /// error rather than a silently mislabeled envelope.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Map { .. } => "map",
            Command::Collide { .. } => "collide",
            Command::Status { .. } => "status",
            Command::Log { .. } => "log",
            Command::History { .. } => "history",
            Command::Diff { .. } => "diff",
            Command::Show { .. } => "show",
            Command::Evolog { .. } => "evolog",
            Command::Git { .. } => "git",
            Command::Restore { .. } => "restore",
            Command::Trim { .. } => "trim",
            Command::Commit { .. } => "commit",
            Command::Switch { .. } => "switch",
            Command::Undo => "undo",
            Command::Redo => "redo",
            Command::Op { action } => action.name(),
            // Bare `ff branch` is the list, so it names the shape it emits
            // rather than the family — two payloads under one name is what
            // the `ff op` family was built to avoid.
            Command::Branch { action, .. } => {
                action.as_ref().map_or("branch list", BranchAction::name)
            }
            Command::Start { .. } => "start",
            Command::Describe { .. } => "describe",
            Command::Absorb { .. } => "absorb",
            Command::Lift { .. } => "lift",
            Command::Restack { .. } => "restack",
            Command::Sync { .. } => "sync",
            Command::Publish { .. } => "publish",
            Command::Remote => "remote",
            Command::Init { .. } => "init",
            Command::Clone { .. } => "clone",
            Command::Edit { .. } => "edit",
            Command::Done { .. } => "done",
            Command::Resolve { .. } => "resolve",
            Command::Hook { .. } => "hook",
            Command::Config { .. } => "config",
            Command::Doctor { .. } => "doctor",
            Command::Explain { .. } => "explain",
            Command::Version => "version",
            Command::Update { .. } => "update",
            // A foreign verb only ever fails, and the envelope names what was
            // typed rather than what fufu would have run: a script reading
            // `{"cmd":"checkout"}` learns which of its words was the foreign
            // one, which a fufu verb name would have hidden.
            Command::Checkout { .. } => "checkout",
            Command::Stash { .. } => "stash",
            Command::Pull { .. } => "pull",
            Command::Push { .. } => "push",
            Command::Rebase { .. } => "rebase",
            Command::Merge { .. } => "merge",
            Command::Blame { .. } => "blame",
            Command::Tag { .. } => "tag",
        }
    }

    /// The `--at-op` / `--at` pair this verb declared, if it declared one.
    ///
    /// A match arm per variant, so a verb added without deciding whether it
    /// reads a past state is a compile error rather than a flag that silently
    /// does nothing. `None` is the positive statement that the verb only adds
    /// to now — the parser has already refused the flags there.
    pub fn past(&self) -> Option<&Past> {
        match self {
            Command::Status { past }
            | Command::Log { past, .. }
            | Command::Evolog { past, .. }
            | Command::Restore { past, .. } => Some(past),
            // Declared twice on purpose: once for the bare form, once for
            // the spelled-out `list`, so `--at-op` is takeable on either
            // side of the subcommand rather than only before it.
            Command::Branch { action, past } => {
                Some(action.as_ref().and_then(BranchAction::past).unwrap_or(past))
            }
            Command::Op { action } => action.past(),
            // The map declares no past flags, on the same rule as bare `ff`:
            // reading it as of a past operation would need a past-state view
            // that does not exist yet.
            // `ff history` answers "where can I go from now"; placing that
            // at a past operation needs the past-state view that does not
            // exist yet.
            // `ff diff` is the open change, and the open change as of a past
            // operation is precisely what `ff op diff <op>` already answers —
            // so the flag would be a second spelling for a verb we have.
            Command::History { .. }
            | Command::Diff { .. }
            | Command::Show { .. }
            | Command::Map { .. }
            | Command::Collide { .. }
            | Command::Git { .. }
            | Command::Trim { .. }
            | Command::Commit { .. }
            | Command::Switch { .. }
            | Command::Undo
            | Command::Redo
            | Command::Start { .. }
            | Command::Describe { .. }
            | Command::Absorb { .. }
            | Command::Lift { .. }
            | Command::Restack { .. }
            | Command::Sync { .. }
            | Command::Edit { .. }
            | Command::Done { .. }
            | Command::Resolve { .. }
            | Command::Hook { .. }
            | Command::Config { .. }
            | Command::Remote
            | Command::Doctor { .. }
            | Command::Explain { .. }
            | Command::Update { .. }
            // The one verb that reads no repository at all: it reports on the
            // binary, and a past operation has nothing to say about that.
            | Command::Version
            // No past: `--at-op` places a command against an input state, and
            // these two run before there is one.
            | Command::Init { .. }
            | Command::Clone { .. }
            | Command::Checkout { .. }
            | Command::Stash { .. }
            | Command::Publish { .. }
            | Command::Pull { .. }
            | Command::Push { .. }
            | Command::Rebase { .. }
            | Command::Merge { .. }
            | Command::Blame { .. }
            | Command::Tag { .. } => None,
        }
    }

    /// Whether `--json` means anything here. Three verbs own their stream
    /// rather than emit an envelope on it: `git` passes real git's output
    /// through (often by exec'ing it), `hook` speaks the agent client's
    /// protocol on stdout, and `update` narrates a download to a person. For
    /// those the flag is ignored, not honored with an empty envelope.
    pub fn json_capable(&self) -> bool {
        !matches!(
            self,
            Command::Git { .. } | Command::Hook { .. } | Command::Update { .. }
        )
    }

    /// The ambient lanes this verb rides. It lives beside the variant, so a
    /// verb added without deciding its lanes is a compile error rather than a
    /// lane that silently never fires — the same rule `name()` and `past()`
    /// already state.
    pub fn lanes(&self) -> Lanes {
        match self {
            // These ride nothing: `init` and `clone` capture last, by design
            // — there is no repository to capture until their work is done;
            // `update`'s detached child must touch nothing and must not
            // recurse; `git` and `hook` already carry their own lanes, in
            // their own order (git's notice is deliberately deferred past
            // git's output); and the foreign redirects print a redirect and
            // stop — a snapshot for a command that does not exist would be a
            // row on the log for something that never ran.
            Command::Init { .. }
            | Command::Clone { .. }
            | Command::Update { .. }
            | Command::Git { .. }
            | Command::Hook { .. }
            | Command::Checkout { .. }
            | Command::Stash { .. }
            | Command::Pull { .. }
            | Command::Push { .. }
            | Command::Rebase { .. }
            | Command::Merge { .. }
            | Command::Blame { .. }
            | Command::Tag { .. } => Lanes::NONE,
            Command::Version => Lanes::QUIET_UPDATE,
            // No repository in its answer either, but it takes the generic
            // notice — one arm is not a const.
            Command::Explain { .. } => Lanes {
                capture: false,
                update: true,
                notice: true,
                trim: false,
            },
            // The core runs `ops::verb::begin_verb` for every one of these
            // (and for `restore`), whose capture is the mandatory pre-verb
            // one: a second CLI capture in front of it would be a full extra
            // worktree diff on the most expensive commands, and a real
            // regression against the bench gates.
            Command::Commit { .. }
            | Command::Switch { .. }
            | Command::Undo
            | Command::Redo
            | Command::Start { .. }
            | Command::Describe { .. }
            | Command::Absorb { .. }
            | Command::Lift { .. }
            | Command::Restack { .. }
            | Command::Sync { .. }
            | Command::Publish { .. }
            | Command::Done { .. }
            | Command::Resolve { .. }
            | Command::Edit { .. }
            | Command::Restore { .. } => Lanes::MUTATOR,
            // `update_row()` already reports the available release in
            // doctor's own voice, so the generic notice would say it twice.
            Command::Doctor { .. } => Lanes {
                capture: true,
                update: true,
                notice: false,
                trim: true,
            },
            // A dry run deliberately does not stamp; if the auto-trim rode
            // the same invocation it would find the stamp due and perform a
            // real trim — the precise surprise a dry run exists to prevent.
            Command::Trim { .. } => Lanes {
                capture: true,
                update: true,
                notice: true,
                trim: false,
            },
            // The readers: they have a repository, and the snapshot is the
            // only pre-work they owe.
            Command::Map { .. }
            | Command::Collide { .. }
            | Command::Status { .. }
            | Command::Log { .. }
            | Command::History { .. }
            | Command::Diff { .. }
            | Command::Show { .. }
            | Command::Evolog { .. }
            | Command::Config { .. }
            | Command::Remote => Lanes::READ,
            // The two families hold both readers and mutators, so the action
            // decides.
            Command::Op { action } => action.lanes(),
            Command::Branch { action, .. } => {
                action.as_ref().map_or(Lanes::READ, BranchAction::lanes)
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    /// Windows hands a process's main thread 1 MiB of stack; Linux and macOS
    /// hand it 8. `build.rs` reserves 8 on Windows too, so the three agree —
    /// but the reason that reserve exists is that the derived builder below
    /// grew past 1 MiB in one frame, and every `ff` invocation on Windows
    /// died with `STATUS_STACK_OVERFLOW` while all three platforms had been
    /// green a commit earlier.
    ///
    /// This is the guard for the next time. It builds the whole command tree
    /// and parses through it on a deliberately small thread, so growth is
    /// caught here, on every platform, rather than by a Windows-only CI leg
    /// after the fact. The budget is half of what is reserved: a tree that
    /// needs more than that has room to finish the release it is in, and a
    /// deliberate decision to make before the next one.
    const STACK_BUDGET: usize = 4 * 1024 * 1024;

    #[test]
    fn the_command_tree_fits_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(STACK_BUDGET)
            .spawn(|| {
                // Both halves: constructing the tree, and walking it. The
                // derive splits those across different generated functions,
                // and either one is where the frame would grow.
                let mut root = super::Cli::command();
                root.build();
                assert!(root.find_subcommand("diff").is_some());
                super::Cli::try_parse_from(["ff", "diff", "src/"]).expect("parses");
            })
            .expect("spawn")
            .join()
            .expect("the command tree overflowed a small stack — see build.rs");
    }
}
