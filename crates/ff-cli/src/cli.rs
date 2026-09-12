use std::ffi::OsString;
use std::path::PathBuf;

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
    // "ff.exe" on Windows, so usage lines would read "Usage: ff.exe hook"
    // there and "Usage: ff hook" everywhere else. Subcommands inherit it.
    bin_name = "ff",
    version = VERSION,
    // The version line's name, and only that: usage lines keep `bin_name`,
    // which is what you actually type.
    display_name = NAME,
    // Declared by hand below, for the short letter: clap's own flag is `-V`.
    disable_version_flag = true,
    about = "a friendlier interface to plain git",
    long_about = help::term(help::ROOT),
    after_long_help = help::term_examples(help::ROOT_EXAMPLES)
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
    // The fetch lane's two overrides, global like `--json` so they ride any
    // verb. clap's `conflicts_with` holds per parse level and the globals
    // are propagated afterwards, so `ff --fetch status --no-fetch` reaches
    // `settle` with both set — that is where the pair is refused.
    /// Fetch from the remote first, whatever the cadence says
    #[arg(long, global = true, conflicts_with = "no_fetch")]
    pub fetch: bool,
    /// Skip the fetch: read the tracking refs as they stand
    #[arg(long, global = true)]
    pub no_fetch: bool,
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
    // The second short letter above the verbs, and git's spelling of it. The
    // long-only rule for shared flags buys verbs a free letter apiece; this
    // one is bought back because the habit is already in everybody's fingers
    // and an uppercase letter was spoken for anyway (`-V`). The long form is
    // the canonical name, as with `-v`/`--version`.
    //
    // Named for the mechanism: it is a chdir, so a relative path argument
    // after it resolves against <dir> too — the whole command moves. `--repo`
    // would have promised something narrower than what happens.
    /// Run as if fufu had been started in <dir>
    #[arg(short = 'C', long = "cwd", value_name = "dir", global = true)]
    pub cwd: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

// A `// agent notice quotes this` line marks surface an agent is taught
// verbatim — by the once-per-session briefing (`NOTICE` in integ/briefing.rs),
// or by the skill shipped beside it (integ/skill.md). Those two texts are the
// only spelling lessons an agent gets, so a retired verb or a renamed flag in
// either teaches it to fail: change one here and fix it there in the same
// commit. `grep -rn "agent notice" crates/ff-cli/src` finds every site, both
// directions. The briefing keeps the stricter contract — every verb it names
// carries a marker — because it is the text that cannot afford to be wrong.
#[derive(Subcommand)]
pub enum Command {
    /// The map bare `ff` draws: the local branches as a skeleton
    #[command(long_about = help::term(help::ROOT), after_long_help = help::term_examples(help::ROOT_EXAMPLES))]
    Map {
        /// Branches to show, newest tip first; 0 means all
        #[arg(short = 'n', long = "max-count", value_name = "count")]
        branches: Option<usize>,
        /// Every local branch
        #[arg(long)]
        all: bool,
    },
    /// Would two branches hit each other if both landed
    #[command(long_about = help::term(help::COLLIDE), after_long_help = help::term_examples(help::COLLIDE_EXAMPLES))]
    Collide {
        /// The two branches; one name means the branch you are on and that one
        #[arg(num_args = 1..=2, required = true, value_name = "branch")]
        names: Vec<String>,
    },
    // agent notice quotes this: `ff status`
    /// Show the working copy status
    #[command(visible_alias = "st", long_about = help::term(help::STATUS), after_long_help = help::term_examples(help::STATUS_EXAMPLES))]
    Status {
        #[command(flatten)]
        past: Past,
    },
    // agent notice quotes this: `ff log`
    /// Show the timeline: commits wearing the operations that built them
    #[command(long_about = help::term(help::LOG), after_long_help = help::term_examples(help::LOG_EXAMPLES))]
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
        /// Verify each commit's signature and show the status letter — one signer run per row
        #[arg(long, conflicts_with = "commits")]
        signatures: bool,
        /// Files or directories to limit the log to; all of them when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
        #[command(flatten)]
        past: Past,
    },
    // agent notice quotes this: `ff diff`
    /// Show the open change as a patch — content, not just counts
    #[command(long_about = help::term(help::DIFF), after_long_help = help::term_examples(help::DIFF_EXAMPLES))]
    Diff {
        /// Files or directories to limit the patch to; all of them when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    /// Show one commit: what it was, and what it did
    #[command(long_about = help::term(help::SHOW), after_long_help = help::term_examples(help::SHOW_EXAMPLES))]
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
    #[command(long_about = help::term(help::HISTORY), after_long_help = help::term_examples(help::HISTORY_EXAMPLES))]
    History {
        /// Number of undo steps to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
    },
    /// Show a change's operations, newest first (the evolution log)
    #[command(visible_alias = "ev", long_about = help::term(help::EVOLOG), after_long_help = help::term_examples(help::EVOLOG_EXAMPLES))]
    Evolog {
        /// The change to drill into: a change id, a sha, any revision; `@` when omitted
        #[arg(value_name = "rev")]
        rev: Option<String>,
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
    /// Capture-first git passthrough; fufu.gitPolicy decides what it says
    #[command(
        disable_help_flag = true,
        long_about = help::term(help::GIT),
        after_long_help = help::term_examples(help::GIT_EXAMPLES)
    )]
    Git {
        /// Arguments passed to git verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    // agent notice quotes this: `ff restore <path>`, `--all --at <time>`, `--at-op <id>`
    /// Restore worktree files from the timeline
    #[command(long_about = help::term(help::RESTORE), after_long_help = help::term_examples(help::RESTORE_EXAMPLES))]
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
    #[command(long_about = help::term(help::TRIM), after_long_help = help::term_examples(help::TRIM_EXAMPLES))]
    Trim {
        /// Report what would be dropped without writing anything
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Also drop the pointers of branches that no longer exist
        #[arg(long)]
        gone: bool,
    },
    // agent notice quotes this: `ff commit -m`
    /// Close the open change into a commit (the working copy is the change)
    #[command(visible_alias = "ci", long_about = help::term(help::COMMIT), after_long_help = help::term_examples(help::COMMIT_EXAMPLES))]
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
        /// Sign the commit, whatever commit.gpgsign says; the key is user.signingkey
        #[arg(short = 'S', long)]
        sign: bool,
        /// Do not sign the commit, whatever commit.gpgsign says
        #[arg(long, conflicts_with = "sign")]
        no_sign: bool,
        /// Files or directories to close, leaving the rest open; all of it when omitted
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
    // agent notice quotes this: `ff switch <branch>`, `ff start`
    /// Switch branches, or begin new work on a fresh one; a dirty tree is parked, a parked change resumes
    #[command(
        visible_aliases = ["sw", "start", "new"],
        long_about = help::term(help::SWITCH),
        after_long_help = help::term_examples(help::SWITCH_EXAMPLES)
    )]
    Switch {
        /// A branch here, a remote's branch, or a revision; nothing means trunk
        #[arg(value_name = "target")]
        target: Option<String>,
        /// Pending description for the change being opened
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Fork a branch target, or name the minted branch; bare -b goes after the target
        #[arg(short = 'b', value_name = "name", num_args = 0..=1)]
        branch: Option<Option<String>>,
    },
    // agent notice quotes this: `ff undo`
    /// Step the whole repository back one run of work
    #[command(long_about = help::term(help::UNDO), after_long_help = help::term_examples(help::UNDO_EXAMPLES))]
    Undo,
    /// Step forward again after an undo
    #[command(long_about = help::term(help::REDO), after_long_help = help::term_examples(help::REDO_EXAMPLES))]
    Redo,
    /// The operation log as objects: read it, compare it, move to it
    #[command(long_about = help::term(help::OP), after_long_help = help::term_examples(help::OP_EXAMPLES))]
    Op {
        #[command(subcommand)]
        action: OpAction,
    },
    /// Edit the pending description of the open change
    #[command(visible_alias = "desc", long_about = help::term(help::DESCRIBE), after_long_help = help::term_examples(help::DESCRIBE_EXAMPLES))]
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
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
    },
    // agent notice quotes this: `ff absorb --into <rev>`
    /// Fold working changes into a commit that has already closed
    #[command(visible_alias = "squash", long_about = help::term(help::ABSORB), after_long_help = help::term_examples(help::ABSORB_EXAMPLES))]
    Absorb {
        /// Commits to move; without it, the open change
        #[arg(long, value_name = "revset")]
        from: Option<String>,
        /// Commit to move into; without it, the commit under the sources
        #[arg(long, value_name = "rev")]
        into: Option<String>,
        /// The target's message: a reword for a closed commit, the pending description for the open change
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Limit the move to these paths (files or directory prefixes)
        #[arg(value_name = "path")]
        paths: Vec<String>,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
    },
    /// Take changes back out of a closed commit, into the open change
    #[command(long_about = help::term(help::LIFT), after_long_help = help::term_examples(help::LIFT_EXAMPLES))]
    Lift {
        /// Commits to move out of; without it, the commit under the open change
        #[arg(long, value_name = "revset")]
        from: Option<String>,
        /// Where it lands; without it, the open change
        #[arg(long, value_name = "rev")]
        into: Option<String>,
        /// The target's message: a reword for a closed commit, the pending description for the open change
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
        /// Limit the move to these paths (files or directory prefixes)
        #[arg(value_name = "path")]
        paths: Vec<String>,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
    },
    /// Replay a branch's commits onto the base it sits on
    #[command(visible_alias = "rebase", long_about = help::term(help::RESTACK), after_long_help = help::term_examples(help::RESTACK_EXAMPLES))]
    Restack {
        /// Branch to restack; without it, the one you are on
        #[arg(value_name = "branch")]
        branch: Option<String>,
        /// Base to replay onto; recorded as this branch's new parent
        #[arg(long, value_name = "branch")]
        onto: Option<String>,
    },
    /// Land this branch on another and take the branch away
    #[command(long_about = help::term(help::FOLD), after_long_help = help::term_examples(help::FOLD_EXAMPLES))]
    Fold {
        /// Branch to fold into; without it, trunk
        #[arg(value_name = "branch")]
        target: Option<String>,
        /// Advance a target another worktree holds, there, and keep this branch here
        #[arg(long)]
        stay: bool,
    },
    // agent notice quotes this: `ff pull`
    /// Line every branch up with its base and its remote
    #[command(visible_alias = "sync", long_about = help::term(help::PULL), after_long_help = help::term_examples(help::PULL_EXAMPLES))]
    Pull {
        /// Branches to pull, each with the bases beneath it; without any, the one you are on
        #[arg(value_name = "branch", conflicts_with = "all")]
        branches: Vec<String>,
        /// Every local branch
        #[arg(long)]
        all: bool,
        /// Say what would move, hold, and be skipped, without writing it
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    // agent notice quotes this: `ff push`
    /// Send this branch to its remote, under a lease
    #[command(visible_alias = "publish", long_about = help::term(help::PUSH), after_long_help = help::term_examples(help::PUSH_EXAMPLES))]
    Push {
        /// Branches to push, each under its own lease; without any, the one you are on
        #[arg(value_name = "branch")]
        branches: Vec<String>,
        /// Say which push this would be, without sending it
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Send to this remote, and record that the branch answers to it
        #[arg(long, value_name = "remote")]
        to: Option<String>,
    },
    /// What the remotes here are called, and where each one points
    #[command(long_about = help::term(help::REMOTE), after_long_help = help::term_examples(help::REMOTE_EXAMPLES))]
    Remote,
    /// Start a repository with the safety net already on
    #[command(long_about = help::term(help::INIT), after_long_help = help::term_examples(help::INIT_EXAMPLES))]
    Init {
        /// Where to create it; the current directory when omitted
        #[arg(value_name = "dir")]
        dir: Option<String>,
        /// Refused: a bare repository has no working copy to capture
        #[arg(long, hide = true)]
        bare: bool,
    },
    /// Clone a repository, and arm it on arrival
    #[command(long_about = help::term(help::CLONE), after_long_help = help::term_examples(help::CLONE_EXAMPLES))]
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
    #[command(long_about = help::term(help::EDIT), after_long_help = help::term_examples(help::EDIT_EXAMPLES))]
    Edit {
        /// The commit to edit. A branch name is a switch instead
        #[arg(value_name = "rev")]
        rev: String,
    },
    /// Finish the editing session: amend, replay what waited, land back
    #[command(long_about = help::term(help::DONE), after_long_help = help::term_examples(help::DONE_EXAMPLES))]
    Done {
        /// Drop the session instead of landing it
        #[arg(long)]
        abandon: bool,
        /// Skip pre-commit and commit-msg hooks
        #[arg(long)]
        no_verify: bool,
    },
    /// Materialize a held rewrite's conflicts and fix them, all at once
    #[command(long_about = help::term(help::RESOLVE), after_long_help = help::term_examples(help::RESOLVE_EXAMPLES))]
    Resolve {
        /// Drop the pending rewrite instead of resolving it
        #[arg(long)]
        abandon: bool,
    },
    /// Lines of work: what exists, making one, and removing one
    #[command(visible_aliases = ["br", "bookmark"], long_about = help::term(help::BRANCH), after_long_help = help::term_examples(help::BRANCH_EXAMPLES))]
    Branch {
        /// Create a branch by this name, and stay where you are
        #[arg(value_name = "name", conflicts_with_all = ["delete", "all", "at", "at_op"])]
        name: Option<String>,
        /// Where it forks from: a revision, `@`, or a branch; trunk when omitted
        #[arg(value_name = "rev", requires = "name")]
        rev: Option<String>,
        /// Delete a branch — its timeline moves to trash, and `ff undo` is enough
        #[arg(short = 'd', long = "delete", value_name = "branch", conflicts_with_all = ["all", "at", "at_op"])]
        delete: Option<String>,
        /// Remove the copy on the remote too — that half `ff undo` cannot reach
        #[arg(long, requires = "delete")]
        shared: bool,
        /// Delete every branch whose shared copy is gone, in one operation
        #[arg(long, conflicts_with_all = ["name", "delete", "shared", "all", "at", "at_op"])]
        prune: bool,
        /// Say what --prune would delete and keep, and write nothing
        #[arg(short = 'n', long, requires = "prune")]
        dry_run: bool,
        /// Every remote-only branch, not just the newest few
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        past: Past,
    },
    /// Worktrees of this repository, and the chains of ones that are gone
    #[command(visible_alias = "workspace", long_about = help::term(help::WORKTREE), after_long_help = help::term_examples(help::WORKTREE_EXAMPLES))]
    Worktree {
        /// Make a worktree here: a second checkout of this repository, with its own log
        #[arg(value_name = "path", conflicts_with_all = ["delete", "at", "at_op"])]
        path: Option<PathBuf>,
        /// The branch it stands on — a new one named after the directory if you do not say
        #[arg(value_name = "branch", requires = "path")]
        branch: Option<String>,
        /// Take a worktree away, capturing what it holds first — by path or by the id `ff worktree` shows
        #[arg(short = 'd', long = "delete", value_name = "worktree", conflicts_with_all = ["at", "at_op"])]
        delete: Option<String>,
        #[command(flatten)]
        past: Past,
    },
    /// Hook fufu into the agent clients and shells on this machine
    #[command(long_about = help::term(help::HOOK), after_long_help = help::term_examples(help::HOOK_EXAMPLES))]
    Hook {
        /// Slugs to hook: claude, codex, cursor, gemini, bash, zsh, fish, powershell
        #[arg(value_name = "slug")]
        slugs: Vec<String>,
        /// Everything detected, without asking
        #[arg(long)]
        all: bool,
        /// Report what is here and stop
        #[arg(short = 'l', long = "list")]
        list: bool,
        /// claude only: wire settings entries instead of the plugin
        #[arg(long)]
        settings: bool,
        /// Refresh what is wired: re-run the install for every slug already
        /// wired, adding none
        #[arg(short = 'u', long = "update", conflicts_with_all = ["slugs", "all", "list", "settings", "skill"])]
        update: bool,
        /// Print fufu's skill and stop, for a client that reads none
        #[arg(long, conflicts_with_all = ["slugs", "all", "list", "settings"])]
        skill: bool,
    },
    /// Remove exactly what hook added
    #[command(long_about = help::term(help::UNHOOK), after_long_help = help::term_examples(help::UNHOOK_EXAMPLES))]
    Unhook {
        /// Slugs to unhook; none reports and asks
        #[arg(value_name = "slug")]
        slugs: Vec<String>,
        /// Everything detected, without asking
        #[arg(long)]
        all: bool,
    },
    /// Snapshot the working copy now
    #[command(long_about = help::term(help::TRIGGER), after_long_help = help::term_examples(help::TRIGGER_EXAMPLES))]
    Trigger {
        /// The source; absent or `manual` is the hand-taken snapshot
        #[arg(value_name = "source")]
        source: Option<String>,
        /// Say what this snapshot is for
        #[arg(short = 'm', value_name = "msg")]
        message: Option<String>,
    },
    /// Stream operations as they land, one JSON object per line
    #[command(long_about = help::term(help::WATCH), after_long_help = help::term_examples(help::WATCH_EXAMPLES))]
    Watch {
        /// Every worktree in the repository, not just this one
        #[arg(long)]
        all: bool,
        /// Replay from this operation before tailing
        #[arg(long, value_name = "op")]
        since: Option<String>,
        /// Only operations of this kind: capture, op, foreign, note
        #[arg(long, value_name = "kind")]
        kind: Option<String>,
        /// Only operations tagged with this session
        #[arg(long, value_name = "name")]
        session: Option<String>,
        /// Stop after this many events, counting the opening one; 0 means never
        #[arg(short = 'n', long = "max-count", value_name = "count")]
        count: Option<usize>,
    },
    /// Read and write fufu's settings (plain git config under fufu.*)
    #[command(visible_alias = "cfg", long_about = help::term(help::CONFIG), after_long_help = help::term_examples(help::CONFIG_EXAMPLES))]
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
    #[command(long_about = help::term(help::DOCTOR), after_long_help = help::term_examples(help::DOCTOR_EXAMPLES))]
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
    #[command(long_about = help::term(help::VERSION), after_long_help = help::term_examples(help::VERSION_EXAMPLES))]
    Version,
    /// Name the command that updates this fufu, and offer to run it
    #[command(long_about = help::term(help::UPDATE), after_long_help = help::term_examples(help::UPDATE_EXAMPLES))]
    Update {
        /// Refresh the update cache only (used by the background check)
        #[arg(long)]
        check: bool,
        /// Run the update command without asking
        #[arg(short = 'y', long)]
        yes: bool,
    },

    // The foreign verbs: git's and jj's words fufu answers rather than runs.
    // They are declared for the same reason the retired `-m` and `--ops` are
    // — a word fufu deliberately does not have is a question, and clap's
    // bare "unrecognized subcommand" answers a question it never asked.
    // Hidden, because the command list is what fufu *does*; each one carries
    // its own arguments so `ff checkout main` reaches the answer instead of
    // dying on an unexpected argument first. A jj word whose meaning *is* a
    // fufu verb is a visible alias on that verb instead, not a row here.
    /// git's checkout, split in two: `ff switch` moves, `ff restore` brings files back
    #[command(hide = true)]
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
    /// No `ff merge`: fufu replays — `ff restack --onto`, `ff pull` — rather than merging
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
    /// No `ff abandon`: `ff restore --all` drops the open change, `ff lift --from` a closed one
    #[command(hide = true)]
    Abandon {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// No `ff split`: `ff commit <paths>` closes a slice, `ff lift --from <rev> <paths>` reopens one
    #[command(hide = true)]
    Split {
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
    /// Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)
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
    #[command(long_about = help::term(help::OP_LOG), after_long_help = help::term_examples(help::OP_LOG_EXAMPLES))]
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
    #[command(long_about = help::term(help::OP_SHOW), after_long_help = help::term_examples(help::OP_SHOW_EXAMPLES))]
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
    #[command(long_about = help::term(help::OP_DIFF), after_long_help = help::term_examples(help::OP_DIFF_EXAMPLES))]
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
    #[command(long_about = help::term(help::OP_RESTORE), after_long_help = help::term_examples(help::OP_RESTORE_EXAMPLES))]
    Restore {
        /// The operation to land on
        #[arg(value_name = "op")]
        op: String,
        /// Rewind to what remains even if parts were trimmed
        #[arg(long)]
        force: bool,
    },
    /// Invert one operation, leaving later work standing
    #[command(long_about = help::term(help::OP_REVERT), after_long_help = help::term_examples(help::OP_REVERT_EXAMPLES))]
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
    ///
    /// None of them reads anything under `refs/remotes/`, so none carries
    /// the fetch.
    fn lanes(&self) -> Lanes {
        match self {
            OpAction::Log { .. } | OpAction::Show { .. } | OpAction::Diff { .. } => {
                Lanes::READ.without_fetch()
            }
            OpAction::Restore { .. } | OpAction::Revert { .. } => Lanes::MUTATOR.without_fetch(),
        }
    }
}

/// Whether the fetch lane rides a verb, and on what clock. In the table
/// rather than a special case in `main`, so doctor's every-run fetch is a
/// value beside the others and not a `matches!` somebody has to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    /// The verb reads nothing under `refs/remotes/`, or owns its own fetch.
    Off,
    /// A fetch rides the invocation when `fufu.autoFetch` says one is due.
    Cadence,
    /// A fetch rides every invocation — doctor, whose rows are the remote
    /// floor as it stands right now. `fufu.autoFetch false` still turns it
    /// off; only `--fetch` runs past that.
    Every,
}

/// The ambient lanes: what rides an invocation besides the verb itself —
/// the pre-command capture, the passive update lane (cache refresh,
/// auto-install, the one-line notice), the daily auto-trim, and the fetch
/// on `fufu.autoFetch`'s cadence. One table on `Command` decides them all,
/// so a verb is in it by construction rather than by somebody remembering
/// to call four functions at the bottom of its `run`.
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
    /// Let a fetch ride this invocation, before the verb reads the tracking refs.
    pub fetch: Fetch,
}

impl Lanes {
    /// Nothing at all: the verbs that run before there is a repository, the
    /// detached `update` child, `git` and the wiring verbs (each already carries
    /// its own lanes on purpose), and the foreign redirects (they print and stop).
    pub const NONE: Lanes = Lanes {
        capture: false,
        update: false,
        notice: false,
        trim: false,
        fetch: Fetch::Off,
    };

    /// A reader: the snapshot is the only pre-work it owes.
    pub const READ: Lanes = Lanes {
        capture: true,
        update: true,
        notice: true,
        trim: true,
        fetch: Fetch::Cadence,
    };

    /// A mutator: the core already captures, so a second one at the CLI
    /// would be a full extra worktree diff on the most expensive commands.
    pub const MUTATOR: Lanes = Lanes {
        capture: false,
        update: true,
        notice: true,
        trim: true,
        fetch: Fetch::Cadence,
    };

    /// `version`: the update lane with its voice removed — it prints its own
    /// "available" line, and it has no repository in its answer.
    pub const QUIET_UPDATE: Lanes = Lanes {
        capture: false,
        update: true,
        notice: false,
        trim: false,
        fetch: Fetch::Off,
    };

    /// The same lanes with the fetch turned off.
    pub const fn without_fetch(self) -> Lanes {
        Lanes {
            fetch: Fetch::Off,
            ..self
        }
    }
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
            Command::Branch {
                name,
                delete,
                prune,
                ..
            } => match (name, delete, prune) {
                (_, _, true) => "branch prune",
                (_, Some(_), false) => "branch delete",
                (Some(_), None, false) => "branch create",
                (None, None, false) => "branch list",
            },
            // The same rule as bare `ff branch`: name the shape it emits,
            // not the family.
            Command::Worktree { path, delete, .. } => match (path, delete) {
                (_, Some(_)) => "worktree remove",
                (Some(_), None) => "worktree add",
                (None, None) => "worktree list",
            },
            Command::Describe { .. } => "describe",
            Command::Absorb { .. } => "absorb",
            Command::Lift { .. } => "lift",
            Command::Restack { .. } => "restack",
            Command::Fold { .. } => "fold",
            Command::Pull { .. } => "pull",
            Command::Push { .. } => "push",
            Command::Remote => "remote",
            Command::Init { .. } => "init",
            Command::Clone { .. } => "clone",
            Command::Edit { .. } => "edit",
            Command::Done { .. } => "done",
            Command::Resolve { .. } => "resolve",
            Command::Hook { .. } => "hook",
            Command::Unhook { .. } => "unhook",
            Command::Trigger { .. } => "trigger",
            Command::Watch { .. } => "watch",
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
            Command::Merge { .. } => "merge",
            Command::Blame { .. } => "blame",
            Command::Tag { .. } => "tag",
            Command::Abandon { .. } => "abandon",
            Command::Split { .. } => "split",
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
            | Command::Restore { past, .. }
            // The list's flags: the parser has already refused them next to
            // a positional or `-d`, so a mutator never sees them set.
            | Command::Branch { past, .. }
            | Command::Worktree { past, .. } => Some(past),
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
            | Command::Watch { .. }
            | Command::Git { .. }
            | Command::Trim { .. }
            | Command::Commit { .. }
            | Command::Switch { .. }
            | Command::Undo
            | Command::Redo
            | Command::Describe { .. }
            | Command::Absorb { .. }
            | Command::Lift { .. }
            | Command::Restack { .. }
            | Command::Fold { .. }
            | Command::Pull { .. }
            | Command::Edit { .. }
            | Command::Done { .. }
            | Command::Resolve { .. }
            | Command::Hook { .. }
            | Command::Unhook { .. }
            | Command::Trigger { .. }
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
            | Command::Push { .. }
            | Command::Merge { .. }
            | Command::Blame { .. }
            | Command::Tag { .. }
            | Command::Abandon { .. }
            | Command::Split { .. } => None,
        }
    }

    /// Whether `--json` means anything here. Four verbs own their stream
    /// rather than emit an envelope on it: `git` passes real git's output
    /// through (often by exec'ing it), a client `trigger` speaks that client's
    /// protocol on stdout, `update` talks an install command over with a
    /// person, and
    /// `watch` emits a *stream* of envelopes rather than one, so a flag
    /// asking for JSON would be describing what it already always does. For
    /// those the flag is ignored, not honored with an empty envelope.
    pub fn json_capable(&self) -> bool {
        match self {
            Command::Git { .. } | Command::Update { .. } | Command::Watch { .. } => false,
            // `trigger` is two things under one name, and only one of them
            // owns its stream. The manual snapshot is a verb like any
            // other and emits an envelope; a client source's stdout
            // belongs to that client's protocol, and an envelope on it
            // would be fufu talking over the briefing.
            Command::Trigger { source, .. } => {
                matches!(source.as_deref(), None | Some(crate::integ::manual::SOURCE))
            }
            _ => true,
        }
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
            // recurse; `git` and the trigger sources already carry their own lanes, in
            // their own order (git's notice is deliberately deferred past
            // git's output); and the foreign redirects print a redirect and
            // stop — a snapshot for a command that does not exist would be a
            // row on the log for something that never ran.
            Command::Init { .. }
            | Command::Clone { .. }
            | Command::Update { .. }
            | Command::Git { .. }
            | Command::Hook { .. }
            | Command::Unhook { .. }
            | Command::Trigger { .. }
            | Command::Checkout { .. }
            | Command::Stash { .. }
            | Command::Merge { .. }
            | Command::Blame { .. }
            | Command::Tag { .. }
            | Command::Abandon { .. }
            | Command::Split { .. } => Lanes::NONE,
            // `watch` must ride nothing, and neither half of that is
            // optional. `Lanes::READ` sets `capture: true`, and the capture
            // fires in `lanes::preflight` *before* dispatch — so a watch on
            // READ would append an operation and then stream an event about
            // its own startup. READ also carries the daily auto-trim, which
            // rewrites every id on the log: a watch would be triggering, on
            // its own trailer, the one motion that ends the stream.
            Command::Watch { .. } => Lanes::NONE,
            Command::Version => Lanes::QUIET_UPDATE,
            // No repository in its answer either, but it takes the generic
            // notice — one arm is not a const.
            Command::Explain { .. } => Lanes {
                capture: false,
                update: true,
                notice: true,
                trim: false,
                fetch: Fetch::Off,
            },
            // The core runs `ops::verb::begin_verb` for every one of these
            // (and for `restore`), whose capture is the mandatory pre-verb
            // one: a second CLI capture in front of it would be a full extra
            // worktree diff on the most expensive commands, and a real
            // regression against the bench gates.
            Command::Commit { .. }
            | Command::Switch { .. }
            | Command::Describe { .. }
            | Command::Absorb { .. }
            | Command::Lift { .. }
            | Command::Restack { .. }
            | Command::Fold { .. }
            | Command::Push { .. }
            | Command::Done { .. }
            | Command::Resolve { .. }
            | Command::Edit { .. }
            | Command::Restore { .. } => Lanes::MUTATOR,
            // `pull` owns its fetch and stamps the cadence when it runs;
            // `undo` and `redo` read nothing under `refs/remotes/`.
            Command::Undo | Command::Redo | Command::Pull { .. } => Lanes::MUTATOR.without_fetch(),
            // `update_row()` already reports the available release in
            // doctor's own voice, so the generic notice would say it twice.
            // The remote floor is what doctor checks, so its fetch is every
            // run's, not the cadence's.
            Command::Doctor { .. } => Lanes {
                capture: true,
                update: true,
                notice: false,
                trim: true,
                fetch: Fetch::Every,
            },
            // A dry run deliberately does not stamp; if the auto-trim rode
            // the same invocation it would find the stamp due and perform a
            // real trim — the precise surprise a dry run exists to prevent.
            Command::Trim { .. } => Lanes {
                capture: true,
                update: true,
                notice: true,
                trim: false,
                fetch: Fetch::Cadence,
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
            | Command::Evolog { .. } => Lanes::READ,
            // Readers with nothing of the remote's in their answer: `config`
            // reads a file, `remote` lists names and URLs.
            Command::Config { .. } | Command::Remote => Lanes::READ.without_fetch(),
            // The families that hold both readers and mutators.
            Command::Op { action } => action.lanes(),
            // The two families hold both readers and mutators, so the shape
            // decides: bare is the list and reads; a positional or `-d`
            // runs a core verb whose `begin_verb` capture is the mandatory
            // pre-verb one, and a second CLI capture would be a full extra
            // worktree diff.
            // `--prune` owns its fetch the way `pull` does, and stamps the
            // cadence when it runs.
            Command::Branch { prune: true, .. } => Lanes::MUTATOR.without_fetch(),
            Command::Branch { name, delete, .. } => {
                if name.is_some() || delete.is_some() {
                    Lanes::MUTATOR
                } else {
                    Lanes::READ
                }
            }
            Command::Worktree { path, delete, .. } => {
                if path.is_some() || delete.is_some() {
                    Lanes::MUTATOR
                } else {
                    Lanes::READ
                }
            }
        }
    }
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
