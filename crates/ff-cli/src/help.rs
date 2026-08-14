//! Every help page fufu prints. The prose lives here rather than in
//! `cli.rs` doc comments for one mechanical reason: clap_derive joins a
//! doc comment's lines into a single paragraph and this build has no
//! `wrap_help`, so a doc comment prints as one very long line, while a
//! `&'static str` is emitted line for line. Hand-wrapped at 72 columns.
//!
//! Two consts per command: the long description clap prints above
//! `Usage:` (`long_about`), and the examples it prints below the options
//! (`after_long_help`). The one-line `about` stays in `cli.rs`, where it
//! is also the row in the parent's command list.

pub const ROOT: &str = "\
a friendlier interface to plain git

fufu snapshots your working tree as you work — before every command it
runs, before every git command you type through the alias, before every
tool call your agent makes — so the last hour of work is always
recoverable. Snapshots are ordinary git objects under refs/fufu/, beside
your history rather than in it: nothing fufu stores reaches a remote, and
nothing it stores needs fufu to read back.

Bare `ff` is the snapshot verb: `ff` takes one now, `ff -m \"msg\"` names it.
Every other command captures first, then does its work.";

pub const ROOT_EXAMPLES: &str = "\
Examples:
  ff                             snapshot the working tree right now
  ff -m \"before the refactor\"    snapshot, with a name you will recognize
  ff log                         the timeline: commits wearing their snapshots
  ff restore --at 2h src/        a directory, as it was two hours ago
  ff undo                        roll the whole repo back one operation

Wire it in, so capture is ambient rather than remembered:
  ff hook shell install          alias git='ff git' — git snapshots first
  ff hook agent install          snapshot around your agent's tool calls
  ff doctor                      is any of this actually on?

`ff help <command>` (or `ff <command> --help`) has the details.";

pub const STATUS: &str = "\
Where you are and what is uncommitted: the branch, its upstream, the open
change, and the files that differ from the commit underneath it.

Status is also where drift is loud. Work done behind fufu's back — a plain
`git commit`, a rebase run by a tool that never heard of fufu — is absorbed
into the operation journal lazily, and status keeps reporting it until the
next fufu operation, so foreign motion is never silent.";

pub const STATUS_EXAMPLES: &str = "\
Examples:
  ff status
  ff status --json               the same state, for scripts";

pub const LOG: &str = "\
The changes view, jj-style: the open change (@) sits atop the commit walk
(●), and each commit wears the id of its newest snapshot — the letters
column `ff evolog` drills into.

--commits drops to plain history, no snapshot identity. --ops shows the
operation journal instead: every mutation fufu has made, newest first,
carrying the op ids `ff undo` takes.

The log family pages on a terminal, git-style — fufu.pager, then FF_PAGER,
then PAGER, then less. Piped output and --json never page.";

pub const LOG_EXAMPLES: &str = "\
Examples:
  ff log                         the last 25 rows
  ff log -n 0                    all of it
  ff log --commits               history only, no snapshot rows
  ff log --ops                   the operation journal, with ids for ff undo";

pub const EVOLOG: &str = "\
Every snapshot of the change you have open, newest first — the drill-in
behind the letters column in `ff log`. This is where a lost hour is found:
each row is a whole worktree, and `ff restore --at <id>` brings any of them
back.

Because fufu captures before it works, the newest row is often this
command's own snapshot, taken a moment ago when it found the tree dirty.
That is intended.

Ids are spelled in the letters k–z, never hex digits, so a snapshot id can
never be misread as a commit sha. The bold prefix is the shortest one
`ff restore --at` resolves unambiguously.";

pub const EVOLOG_EXAMPLES: &str = "\
Examples:
  ff evolog                      the open change's snapshots
  ff evolog -n 0                 all of them
  ff restore --at <id> src/      pull a directory back from one";

pub const GIT: &str = "\
Snapshots first, then runs the git command. This is what the shell alias
runs — `alias git='ff git'`, installed by `ff hook shell install` — so
typed git keeps working exactly as it did, and simply stops being able to
lose anything.

Invocations whose meaning maps completely onto a fufu verb are translated,
so muscle memory gets fufu's guarantees without retraining: `git switch`
engages tree memory, `git commit -m` cuts from the capture stream. The
whitelist is deliberately strict — any flag or form fufu does not fully
understand falls through to real git, verbatim and unmodified, still
capture-first.

Every flag here belongs to git, including --help. This page is `ff help
git`.";

pub const GIT_EXAMPLES: &str = "\
Examples:
  ff git status                  translated: this is ff status
  ff git commit -m \"…\"           translated: closes the open change
  ff git rebase -i HEAD~3        not translated: snapshot, then real git
  ff hook shell install          make every typed git command do this";

pub const RESTORE: &str = "\
Files come back as they were in a snapshot — the newest one unless --at
names another. --all restores the whole tree, including deleting files
that were created since.

Only the worktree is written. The index, HEAD, branches, and staged
changes stay exactly as they are. Restore takes its own snapshot first, and
that one is mandatory: if the pre-restore capture fails, nothing is
written. So any restore is undone by another restore, or by `ff undo`.

--at takes a snapshot id as `ff evolog` prints it (letters, or raw hex), a
position on the snapshot timeline (`@{1}` is one snapshot back, `@{2}` two
— git's reflog syntax, counted on snapshots; keep the quotes, some shells
eat braces), a compact age (30m, 2h, 3d, 1w), or any date git understands.";

pub const RESTORE_EXAMPLES: &str = "\
Examples:
  ff restore src/main.rs         the newest snapshot's copy of one file
  ff restore --all --at 2h       the whole tree, as it stood two hours ago
  ff restore --at qkzm docs/     a directory, from a snapshot id
  ff restore --at '@{1}' .       everything, one snapshot back";

pub const TRIM: &str = "\
Retention with an undo. Each chain's pre-trim tip is written to
refs/fufu/trash/<branch> before a single ref moves, so the last trim is
itself recoverable. Survivors keep their trees, messages, and dates
byte-for-byte — only parent slots relink — and the reflog is replayed with
the original times, so `@{1}` and `--at 2h` stay truthful afterwards.

You rarely need to run this. A trim rides an ff command at most once per
fufu.autoTrim (daily by default), per repository. This is the hand-run
form, and the only one that nudges git's own gc when it dropped something.";

pub const TRIM_EXAMPLES: &str = "\
Examples:
  ff trim -n                     preview: what would go, nothing written
  ff trim                        drop everything past the keep window
  ff trim --gone                 also drop chains whose branch is gone
  ff config keep 30d             a shorter window
  ff config autoTrim false       leave trimming entirely to this command";

pub const COMMIT: &str = "\
There is no staging step: the working tree is the change, and closing it
is the commit. -m describes what is closing and wins over any pending
description left by `ff describe`. -b lands the close on a branch — it
claims the anonymous branch you are standing on, or forks a fresh one from
here, leaving the branch you were on where it was.

A described change with no file changes closes as an empty commit, on
purpose; an undescribed clean tree is simply nothing to do. Every close is
journaled, so `ff undo` takes it back — tree and refs together.";

pub const COMMIT_EXAMPLES: &str = "\
Examples:
  ff commit -m \"parser: handle unicode escapes\"
  ff commit                      close with the pending description
  ff commit -b unicode-cleanup   claim the name as the work lands
  ff commit --no-verify          skip pre-commit and commit-msg hooks";

pub const SWITCH: &str = "\
Branches without the stash dance. Whatever is open here is parked with the
branch you are leaving, and whatever was parked where you are going comes
back exactly as you left it — same files, same edits, same pending
description. Both halves are reported, so you always know where your work
went and what came back.

The target is a branch name, or any unique prefix of one. An ambiguous
prefix is an error that lists the candidates.";

pub const SWITCH_EXAMPLES: &str = "\
Examples:
  ff switch main
  ff switch uni                  a unique prefix is enough
  ff undo                        changed your mind: the park and the move
                                 both roll back together";

pub const UNDO: &str = "\
Whole-repo undo: refs and the working tree together, not one without the
other. Operations come from the journal — `ff log --ops` prints them with
the ids this command takes — and the newest undoable one is the default.

There is no confirmation prompt, deliberately: an undo is itself one undo
away, and redo is undoing the undo.

--force rolls back what remains when parts of the pre-state have already
been trimmed, skipping the missing pieces with warnings instead of
refusing outright.";

pub const UNDO_EXAMPLES: &str = "\
Examples:
  ff undo                        take back the last operation
  ff log --ops                   find an older one
  ff undo 3f1c8a2                roll back to before that op
  ff undo                        …and that undo was itself an op: redo";

pub const START: &str = "\
Begin a new line of work on a fresh branch. `ff commit` records, `ff switch`
resumes, `ff start` begins.

Bare `ff start` forks from trunk; a `<rev>` argument forks there instead. A
branch name forks at that branch's tip rather than continuing it — continuing
is `ff switch`'s job.

The open change parks where it was; the new branch opens clean. Nothing is
ever carried across a fork. -m describes the change being *opened*; -b names
the minted branch, else it is anonymous.

`ff start` never creates a commit.";

pub const START_EXAMPLES: &str = "\
Examples:
  ff start                       begin new work, forked from trunk
  ff start -m \"the next thing\"   …with the new change already described
  ff start -b hotfix             name the branch at birth
  ff start 5b7a90e               fork from a specific commit";

pub const DESCRIBE: &str = "\
The open change carries a description before it is ever a commit, so you
can name work while you are doing it and let `ff commit` pick the name up
when it closes. -m sets it inline; the bare form opens $EDITOR seeded with
the current text — the same spawn git makes for a commit message, and one
of the very few fufu makes at all.

-b renames the current branch instead: how an anonymous branch earns a
real name once the work has one.";

pub const DESCRIBE_EXAMPLES: &str = "\
Examples:
  ff describe -m \"parser: handle unicode escapes\"
  ff describe                    open $EDITOR on the pending description
  ff describe -b unicode-cleanup rename the branch you are on";

pub const BRANCH: &str = "\
Bare `ff branch` lists what exists, named branches and anonymous ones kept
apart. Given a name, it claims the anonymous branch you are standing on —
the capture chain and any parked change come along, so claiming a name
costs nothing and loses nothing.

-d deletes. The branch's timeline moves to trash rather than evaporating,
and the whole delete is journaled, so `ff undo` brings the branch and its
snapshots back.";

pub const BRANCH_EXAMPLES: &str = "\
Examples:
  ff branch                      what exists, and what is still anonymous
  ff branch unicode-cleanup      claim the name you are standing on
  ff branch -d old-experiment    delete it (undoable)";

pub const HOOK: &str = "\
Everything that feeds the capture floor is a hook, under one grammar:
ff hook <agent|shell|editor> <install|uninstall|list|trigger> [name].

Hooks are what make capture ambient instead of something you remember.
With none of them wired, fufu snapshots only when you type an ff command —
which works, and misses the whole point. `ff doctor` warns when nothing at
all feeds capture, because a silent engine feels safe while capturing
nothing.";

pub const HOOK_EXAMPLES: &str = "\
Examples:
  ff hook shell install          alias git='ff git' in your shell's rc file
  ff hook agent install          snapshot around your agent's tool calls
  ff hook shell list             what is wired, and where
  ff doctor                      check that something is feeding capture";

pub const HOOK_AGENT: &str = "\
Wires fufu into the agent clients on this machine, so a snapshot lands
before every Bash, Edit, Write, and NotebookEdit the agent runs. That is
the difference between \"the agent broke something\" and \"the agent broke
something, and here is the tree from thirty seconds ago\".

Install writes entries into the client's own settings file; uninstall
removes exactly what install added and nothing else; both are idempotent.
`trigger` is the runtime the client invokes with a payload on stdin — not
a command to run by hand. It always exits 0: a hook must never veto an
agent's action.";

pub const HOOK_AGENT_EXAMPLES: &str = "\
Examples:
  ff hook agent install          wire claude on this machine
  ff hook agent list             installed, or not
  ff hook agent uninstall        remove exactly what install added";

pub const HOOK_SHELL: &str = "\
Adds one line to your shell's rc file — alias git='ff git' — so every git
command you type snapshots before it runs. Scripts, IDEs, and GUIs resolve
the real git on PATH and are untouched: the alias scopes fufu to what a
human types, deliberately.

The shell defaults to $SHELL; name one to wire a different one.";

pub const HOOK_SHELL_EXAMPLES: &str = "\
Examples:
  ff hook shell install          wire $SHELL
  ff hook shell install fish     wire a specific one
  ff hook shell list             which shells are wired
  ff hook shell uninstall        take the line back out";

pub const HOOK_EDITOR: &str = "\
Reserved. The slot exists so the grammar is complete; nothing installs
yet. Unknown hook kinds exit 0 silently by design — a hook must never
break the caller that runs it.";

pub const HOOK_TRIGGER: &str = "\
The hook runtime: the client invokes this with a payload on stdin when
something is about to happen. It is not a command to run by hand.

It always exits 0, whatever went wrong — a hook must never veto the
action it fired on. Failures are silent by design; FF_DEBUG=1 makes them
talk.";

pub const CONFIG: &str = "\
No subcommands — arity decides. Bare `ff config` lists every setting with
its value, its meaning, and a (default) marker. A key alone gets it; a key
plus a value sets it; --unset returns it to the default; --global widens
the set or unset to every repo.

Storage is plain git config under fufu.<key>, so `git config fufu.keep`
and fufu can never disagree, and precedence is git's own. What git config
cannot do is tell you what settings exist, what they default to, or
whether a value will parse — and every fufu reader falls back to its
default on a value it cannot read, so a typo'd setting looks set and does
nothing. Values here are validated through the readers' own parsers before
anything touches disk.";

pub const CONFIG_EXAMPLES: &str = "\
Examples:
  ff config                      every setting, defaults marked
  ff config keep                 what the retention window is
  ff config keep 30d             set it, this repo
  ff config --global pager bat   set it, every repo
  ff config --unset autoTrim     back to the default";

pub const DOCTOR: &str = "\
A safety net you cannot inspect is not trustworthy, and every floor of
this one can degrade quietly: a chain moved by something that is not fufu,
a reflog that never got created, the gc guard deleted out of local config,
hooks never installed, a stale binary. Doctor reads the whole net in one
pass — the engine (chains and their ages, the snapshot identity on every
tip, reflogs, the gc guard, journal health and pending foreign drift,
settings validated through the readers' own parsers, a trim preview and
the auto-trim clock), the wiring (agent hooks, the shell alias, and a
warning when nothing at all feeds capture), and the update lane.

Rows come at three levels: ok counts nothing, info is news rather than a
problem, WARN is a finding. Findings drive the exit code — 0 healthy, 1
findings — so CI can gate on it, and --json emits the same rows for
machines.

Read-only by design: doctor reports the drift the journal will absorb and
never absorbs it, takes no snapshot, reconciles nothing. --fix is the one
consented write, and it repairs exactly the two gc reflog-expiry keys.";

pub const DOCTOR_EXAMPLES: &str = "\
Examples:
  ff doctor                      read the net
  ff doctor --fix                repair the gc keys (the only write)
  ff doctor --json               the same rows, for machines";

pub const UPDATE: &str = "\
Moves the running binary to the latest release: picks this platform's
asset, streams it through sha256 against the release's checksums.txt, and
atomically renames it over the executable. Installs that are not fufu's to
touch get pointed at their own updater instead — Homebrew at `brew upgrade
fufu`, source builds at `cargo install`.

Official builds also keep themselves fresh without being asked. A check
runs at most once per fufu.updateCheck (daily by default), and a newer
release either installs itself silently in the background (fufu.autoUpdate,
on by default) or lands a one-line notice on stderr instead. A release is
announced at most once, ever.

--check is that background lane: it refreshes the cache and prints
nothing.";

pub const UPDATE_EXAMPLES: &str = "\
Examples:
  ff update                      update now
  ff config autoUpdate false     keep checking, but only notice
  ff config updateCheck false    turn the whole lane off";
