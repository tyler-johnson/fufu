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

Bare `ff` is the map: recent work across every branch, parked changes
included — where you left things. It draws the commits that carry shape
(branch tips, forks, merges) and contracts the straight runs between them
into one `~ N commits` row, because the shape is the answer and the
commits are how the shape is labeled.

You never type a capture. Every verb takes one first.

Seven verbs take a short spelling too — st, ci, sw, br, ev, desc, cfg —
for status, commit, switch, branch, evolog, describe, and config.";

pub const ROOT_EXAMPLES: &str = "\
Examples:
  ff                             the map: where you left things
  ff -n 3                        just the three branches you touched last
  ff --all                       every local branch, however old
  ff log                         the timeline: commits wearing their operations
  ff restore src/ --at 2h        a directory, as it was two hours ago
  ff undo                        roll the whole repo back one run of work

Wire it in, so capture reaches the commands you did not type:
  ff hook shell install          alias git='ff git' — git snapshots first
  ff hook agent install          snapshot around your agent's tool calls
  ff doctor                      is any of this actually on?

`ff help <command>` (or `ff <command> --help`) has the details.";

pub const STATUS: &str = "\
Where you are and what is uncommitted: the branch, its upstream, the open
change, and the files that differ from the commit underneath it.

Status is also where drift is loud. Work done behind fufu's back — a plain
`git commit`, a rebase run by a tool that never heard of fufu — is absorbed
into the operation log lazily, and status keeps reporting it until the
next fufu operation, so foreign motion is never silent.";

pub const STATUS_EXAMPLES: &str = "\
Examples:
  ff status
  ff status --json               the same state, for scripts";

pub const LOG: &str = "\
The changes view, jj-style: the open change (@) sits atop the commit walk
(●), and each commit wears the id of its newest operation — the letters
column `ff evolog` drills into.

--commits drops to plain history, no operation identity. The operation log
itself is `ff op log`: every mutation fufu has made, newest first, carrying
the ids the `ff op` verbs take.

-r takes a revset and replaces where the rows come from: gitrevisions'
whole revision grammar, plus a set algebra spelled | & ~ .. and :: . The @
row appears only when the open change is a member of the set, because
`ff log -r main` is a question about main.

The log family pages on a terminal, git-style — fufu.pager, then FF_PAGER,
then PAGER, then less. Piped output and --json never page.";

pub const LOG_EXAMPLES: &str = "\
Examples:
  ff log                         the last 25 rows
  ff log -n 0                    all of it
  ff log --commits               history only, no operation rows
  ff log -r main                 just main's tip — no @ row, it is not in it
  ff log -r 'trunk..@'           what this branch has that trunk does not
  ff op log                      the operation log, in its own address space";

pub const EVOLOG: &str = "\
Every operation on the change you have open, newest first — the drill-in
behind the letters column in `ff log`. This is where a lost hour is found:
each row is a whole worktree, and `ff restore --at-op <id>` brings any of
them back.

Because fufu captures before it works, the newest row is often this
command's own capture, taken a moment ago when it found the tree dirty.
That is intended.

Ids are spelled in the letters k–z, never hex digits, so an operation id
can never be misread as a commit sha. The bold prefix is the shortest one
`ff op` and `--at-op` resolve unambiguously.";

pub const EVOLOG_EXAMPLES: &str = "\
Examples:
  ff evolog                      the open change's operations
  ff evolog -n 0                 all of them
  ff restore src/ --at-op <id>   pull a directory back from one";

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
Files come back as they were somewhere else. Bare, that somewhere is the
commit under the open change — the everyday \"discard my edits to this
file\". --all restores the whole tree, including deleting files that were
created since.

Three flags name a different source, one kind each, because a position
argument has exactly one kind and a second kind takes a flag:

  --from <rev>      a revision — a branch, a sha, any revset naming one
  --at-op <op>      an operation, by its letters-spelled id
  --at <time>       the operation current at a time (30m/2h/3d, or a date)

Only the worktree is written. The index, HEAD, and branches stay exactly
as they are. Restore takes its own capture first, and that one is
mandatory: if the pre-restore capture fails, nothing is written. So any
restore is undone by another restore, or by `ff undo`.";

pub const RESTORE_EXAMPLES: &str = "\
Examples:
  ff restore src/main.rs         discard edits: back to the commit below
  ff restore --all --at 2h       the whole tree, as it stood two hours ago
  ff restore docs/ --at-op kqzm  a directory, from one operation
  ff restore src/ --from main~2  the same paths, from history instead";

pub const TRIM: &str = "\
Retention with an undo. The log's pre-trim tip is written to
refs/fufu/trash/@ops before a single ref moves, so the last trim is itself
recoverable. Survivors keep their trees, messages, and dates
byte-for-byte — only parent slots relink — and the reflog is replayed with
the original times, so `--at 2h` stays truthful afterwards.

You rarely need to run this. A trim rides an ff command at most once per
fufu.autoTrim (daily by default), per repository. This is the hand-run
form, and the only one that nudges git's own gc when it dropped something.";

pub const TRIM_EXAMPLES: &str = "\
Examples:
  ff trim -n                     preview: what would go, nothing written
  ff trim                        drop everything past the keep window
  ff trim --gone                 also drop pointers whose branch is gone
  ff config keep 30d             a shorter window
  ff config autoTrim false       leave trimming entirely to this command";

pub const COMMIT: &str = "\
There is no staging step: the working tree is the change, and closing it
is the commit. -m describes what is closing and wins over any pending
description left by `ff describe`. -b lands the close on a branch — it
claims the anonymous branch you are standing on, or forks a fresh one from
here, leaving the branch you were on where it was.

A clean tree has nothing to close either way: a description does not
make one — it waits for the next close instead. Every close is recorded,
so `ff undo` takes it back — tree and refs together.";

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
other. It takes no argument and repeats — each one goes one step further
back.

A step is a *run*, not an operation. Captures happen at machine rate and a
person's undo does not, so undo steps over the longest stretch of adjacent
captures carrying the same session, ending at the first operation that is
not one. A verb's operation is a decision somebody made, so it is always
its own step — a switch and a commit are two undos, never one — which is
also what keeps undo from rolling past a commit by accident.

Undo moves the log's pointer rather than appending, so the log records
work and never navigation, and `ff redo` is what comes forward again.
Nothing is discarded: what an undo steps off stays reachable, with the
capture taken just before it at the head, so redo hands back the work you
were holding first.

Naming one operation instead of a run is `ff op restore <op>`.";

pub const UNDO_EXAMPLES: &str = "\
Examples:
  ff undo                        step back one run of work
  ff undo                        …and again, further back
  ff redo                        forward again
  ff op log                      what the log holds, with ids
  ff op restore kqzm             land on one named operation instead";

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

-b names the branch you are on instead — the same act whether it is an
anonymous petname earning a real name or a chosen name being replaced. The
capture chain, any parked change, and the pending description all come
along, which is the part a bare `git branch -m` would orphan.

Naming a revision rewords a commit that has already closed instead. Everything
above it re-parents in the same operation, so any branches sitting inside that
range come along with it.";

pub const DESCRIBE_EXAMPLES: &str = "\
Examples:
  ff describe -m \"parser: handle unicode escapes\"
  ff describe                    open $EDITOR on the pending description
  ff describe -b unicode-cleanup name the branch you are on
  ff describe HEAD~2 -m \"fix\"    reword a closed commit, restacking above it";

pub const ABSORB: &str = "\
Folds the open change into a commit that has already closed — the revision
you name, or the one it sits on when you name none. An absorb does not
attribute hunks: the change is the unit, and a path filter only chooses
which of its files fold in, leaving the rest open.

Everything above the target re-parents in the same operation, so a branch
inside that range comes along with it. What moves is the commit's identity
and the stack above it; no file is copied or renamed in the re-point.";

pub const ABSORB_EXAMPLES: &str = "\
Examples:
  ff absorb                      fold everything open into the commit under it
  ff absorb --into HEAD~2        fold it into a commit further back
  ff absorb src/parser.rs        fold only that path";

pub const LIFT: &str = "\
The other direction of absorb: takes paths out of a commit that has already
closed and back into the open change — the revision you name, or the one it
sits on when you name none. A lift does not attribute hunks either: whole
files are what come back out, and a path filter only chooses which of the
commit's files they are.

Everything above the target re-parents in the same operation, so a branch
inside that range comes along with it. If the lift takes everything the
commit held, the commit is dropped, because fufu writes no empty commit.
What moves is the commit's identity and the stack above it; no file is
copied or renamed in the re-point.";

pub const LIFT_EXAMPLES: &str = "\
Examples:
  ff lift                        take everything out of the commit under it
  ff lift --from HEAD~2          take it out of a commit further back
  ff lift src/parser.rs          take only that path back out";

pub const RESTACK: &str = "\
Replays a branch's commits onto the base it sits on — the branch it was
forked from when one was recorded, trunk otherwise. `--onto` records a new
parent first, which is how a branch is re-aimed and the only way to
change it.

The positional names the branch being moved, so a branch you are not
standing on restacks without touching a file on disk. Branches inside
the replayed range come along with it, and a replay that would conflict
stops with nothing changed rather than leaving you mid-rebase.

Offline — it never reaches the network.";

pub const RESTACK_EXAMPLES: &str = "\
Examples:
  ff restack                     replay onto the base this branch sits on
  ff restack feature             restack a branch you are not standing on
  ff restack --onto release-1.2  re-aim this branch and replay onto it";

pub const EDIT: &str = "\
Opens an editing session on a commit: a branch is minted at the commit and you
switch to it, so the commit's real content is what gets edited, with your whole
toolchain pointed at it.

The branch you came from stays exactly where it stands, its commits waiting
ahead. `ff done` amends the commit with what you changed and replays them onto
it. A branch name is a switch instead — the one available reading is taken and
announced. Your open change parks where you stood and comes back when the
session ends.";

pub const EDIT_EXAMPLES: &str = "\
Examples:
  ff edit 3f2a1b                 open a session on that commit
  ff edit HEAD                   edit the commit you are sitting on
  ff edit main                   a branch is a switch, not a session";

pub const DONE: &str = "\
Ends the editing session `ff edit` opened: the commit the session was opened
on is amended with what the working tree now holds, what waited ahead is
replayed onto it, and you land back on the branch the session left standing.

A replay that would conflict stops with nothing changed rather than leaving you
mid-rewrite. `--abandon` drops the session instead of landing it, stashing
whatever is uncommitted rather than discarding it.

It is one operation — the amend, the replay and the return move together — so
one `ff undo` takes the whole session back.";

pub const DONE_EXAMPLES: &str = "\
Examples:
  ff done                        amend, replay what waited, land back
  ff done --abandon              drop the session, stash what is open";

pub const BRANCH: &str = "\
Bookkeeping for lines of work: `ff branch list` says what exists and
`ff branch delete` takes one away. Bare `ff branch` is the list.

Naming is not here. `ff describe -b <name>` names the branch you are on,
on the same axis as -m — one verb for saying what work is, whether the
subject is the change's description or the branch's name.";

pub const BRANCH_EXAMPLES: &str = "\
Examples:
  ff branch                        what exists, and what is still anonymous
  ff branch delete old-experiment  remove it (undoable)
  ff describe -b unicode-cleanup   name the branch you are on";

pub const BRANCH_LIST: &str = "\
Named branches first, then the anonymous ones fufu minted, kept apart so a
petname never reads as something you chose. Each row carries its tip, the
subject there, and what is hanging off it: a parked change, a pending
description, and how it stands against its upstream.";

pub const BRANCH_LIST_EXAMPLES: &str = "\
Examples:
  ff branch list                 what exists, and what is still anonymous
  ff branch list --json          the same, for a machine";

pub const BRANCH_DELETE: &str = "\
The branch's pointer into the log moves to trash rather than evaporating,
its parked change is demoted to an ordinary stash entry, and the tip stays
pinned by the operation — so nothing is lost and there is no merged-check
to argue with. `ff undo` brings the branch and its timeline back.

The branch's operations themselves stay on the log either way; what goes
is the way in through this name.";

pub const BRANCH_DELETE_EXAMPLES: &str = "\
Examples:
  ff branch delete old-experiment
  ff branch delete ff/misty-owl  an anonymous one you are done with
  ff undo                        put it back";

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
Wires two independent pieces into your shell's rc file. The alias —
alias git='ff git' — so every git command you type snapshots first;
scripts, IDEs, and GUIs resolve the real git on PATH and are untouched, on
purpose. The ambient prompt hook — ff hook shell trigger — so the shell can
tell you what syncing would cost before you ask, speaking only when that
changes.

The shell defaults to $SHELL; name one to wire a different one.";

pub const HOOK_SHELL_EXAMPLES: &str = "\
Examples:
  ff hook shell install          wire $SHELL: the alias and the prompt hook
  ff hook shell install fish     wire a specific one
  ff hook shell list             which shells are wired, and how
  ff hook shell uninstall        take both lines back out";

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
this one can degrade quietly: a log moved by something that is not fufu,
a reflog that never got created, the gc guard deleted out of local config,
hooks never installed, a stale binary. Doctor reads the whole net in one
pass — the engine (the operation log and its age, the fufu identity on its
tip, reflogs, the gc guard, log health and pending foreign drift,
settings validated through the readers' own parsers, a trim preview and
the auto-trim clock), the wiring (agent hooks, the shell alias, and a
warning when nothing at all feeds capture), and the update lane.

Rows come at three levels: ok counts nothing, info is news rather than a
problem, WARN is a finding. Findings drive the exit code — 0 healthy, 1
findings — so CI can gate on it, and --json emits the same rows for
machines.

Read-only by design: doctor reports the drift the log will absorb and
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

pub const REDO: &str = "\
The complement of `ff undo`: step forward again along the branch of the
log an undo stepped off. Takes no argument, and repeats — each one goes
one run further forward, until the log is back where it started.

Redo reads where the operation ref has been, so it can only follow a path
that is still there. New work after an undo forks the log rather than
truncating it: nothing is discarded, but redo stops offering a way forward
it can no longer take, and says so. The forked-off branch keeps its ids,
and `ff op restore` still lands on any of them until trim ages them out.";

pub const REDO_EXAMPLES: &str = "\
Examples:
  ff undo && ff redo             back, and forward again
  ff redo                        …and again, after several undos
  ff op log                      where the log stands now";

pub const OP: &str = "\
The operation log as objects. Every capture and every fufu mutation lands
on one log at refs/fufu/ops, and this is the family that reads and moves
it: `log` lists, `show` and `diff` read, `restore` rewinds the whole
repository to one, and `revert` inverts a single one leaving later work
standing. Deleting operations is `ff trim`'s job and nobody else's.

Operation ids are spelled in the letters k–z and never in hex, which is
what keeps hex meaning \"commit\" everywhere in fufu. `@` is the newest
operation, and git's own first-parent suffixes work on it — `@^` is the
one before, `@~3` three back — because an operation's first parent *is*
the operation before it.

`ff undo` is the everyday shortcut for `ff op restore`, argument-free and
repeatable; most work never needs the long form.";

pub const OP_EXAMPLES: &str = "\
Examples:
  ff op log                      what has happened, newest first
  ff op show @                   what the newest operation did
  ff op diff @^ @                what changed across it
  ff op restore kqzm             rewind the whole repository there
  ff undo                        the same move, one run at a time";

pub const OP_LOG: &str = "\
Every operation, newest first, wearing the ids the `ff op` verbs take.
Captures are left out by default — they outnumber verb operations by more
than an order of magnitude, and an unfiltered log is mostly machine noise.
--captures puts them back.

-r takes the set language over operations: the same operators as `ff log`,
reading the other address space. Ancestry follows the log, so `@^` is the
operation before the newest and `::@` is the whole log. Operations bring
three functions of their own — on_branch(), session() and kind() — and
share latest(), heads() and roots(). Filtering to one session is
`session(<name>)`, and that is the only session filter there is.

--at-op and --at bound the walk at a past operation rather than the tip,
so `ff op log --at 2h` is the log as it read two hours ago, and an -r
alongside them is evaluated against that bounded log.

The bold prefix on each id is the shortest one these verbs resolve
unambiguously, so an id copied from here never lands on an ambiguity.";

pub const OP_LOG_EXAMPLES: &str = "\
Examples:
  ff op log                      the last 25 operations
  ff op log --captures           every row, captures included
  ff op log -r 'kind(op)'        verb operations only
  ff op log -r 'session(nightly)'  one session's operations
  ff op log -r '~on_branch(main)'  everything that happened elsewhere
  ff log -r 'base(@)'            the commit the newest operation ran on
  ff op log --at 2h              the log as it read two hours ago";

pub const OP_SHOW: &str = "\
One operation in full: what ran, when, on which branch, what it moved, and
the diffstat of the worktree it carries against the operation before it.
Bare `ff op show` reads `@`, the newest.

Every operation has a tree, which is what makes this uniform — a capture
and a close are read the same way, and differ only in whether there are
ref transitions to list.";

pub const OP_SHOW_EXAMPLES: &str = "\
Examples:
  ff op show                     the newest operation
  ff op show @^                  the one before it
  ff op show kqzm                by id
  ff op show --json              the same, for machines";

pub const OP_DIFF: &str = "\
What changed in the worktree between two operations. Both are operation
ids; the second defaults to `@`, so a single argument reads \"from there to
now\".

This compares the trees two operations carry, not their ref transitions —
adjacent operations can sit on different branches, and the diff across
that seam reads as the whole worktree being replaced, which is literal
rather than wrong.";

pub const OP_DIFF_EXAMPLES: &str = "\
Examples:
  ff op diff @^ @                what the newest operation changed
  ff op diff kqzm                from that operation to now
  ff op diff kqzm kwzq           between two of them";

pub const OP_RESTORE: &str = "\
Rewind the whole repository to an operation: refs, HEAD, the working tree
and the index together, exactly as that operation recorded them.

It moves the log's pointer rather than appending, so what it steps off
stays reachable and `ff redo` walks back forward along it. Nothing is
discarded and no entry is written saying you navigated — the log records
work, not movement.

--force rewinds to what remains when parts of the recorded state have
already been trimmed, naming each missing piece instead of refusing.

`ff undo` is this verb without an argument, moving one run at a time.";

pub const OP_RESTORE_EXAMPLES: &str = "\
Examples:
  ff op restore kqzm             land on that operation
  ff op restore @~3              three operations back
  ff op restore @ --force        what remains, after a trim took the rest
  ff redo                        undo the rewind";

pub const OP_REVERT: &str = "\
Invert one operation and leave everything after it standing. Where
`ff op restore` rewinds to a moment, this undoes a single change in the
middle of later work.

It is the one verb in this family that *writes* an operation, because
inverting one change while later work stands is itself a thing that
happened, and the log should say so.

An inversion that no longer applies cleanly holds rather than guessing:
the conflict is reported and nothing is written.";

pub const OP_REVERT_EXAMPLES: &str = "\
Examples:
  ff op revert kqzm              take that one change back out
  ff op log                      …and see the revert recorded
  ff undo                        take the revert back too";
