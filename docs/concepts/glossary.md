# Glossary

One or two sentences per term, each linking to the page that owns it.

## A–C

**arm** — Turn fufu on in a repository: write the gc guard that stops `git gc` from expiring fufu's refs, and take the [operation log](snapshots-and-undo.md)'s floor. [`ff init`](../reference/cli/init.md) and [`ff clone`](../reference/cli/clone.md) both arm, and [`ff undo`](../reference/cli/undo.md) reaches back to the moment of arming and no further.

**bay** — A secondary worktree: a second checkout of the same repository, sharing the object store and the branches, with a working copy, an index, HEAD, and an operation chain of its own. [`ff worktree add`](../reference/cli/worktree-add.md) makes one; the [worktrees guide](../guides/worktrees.md) is its story.

**capture** — An automatic [snapshot](snapshots-and-undo.md) of the working copy, taken before every fufu command and around every mutation an agent or editor makes through it, at machine rate. A capture is an operation that moves no ref — the tree alone — and its description is written by fufu, never by a person.

**cascade** — What follows a branch's tip moving: every local branch whose base is that branch is replayed onto its new tip, parent before child, through the whole tree, inside the same operation. Every verb that moves a tip runs one; a replay that conflicts holds that branch and leaves the branches above it alone. [Branches](branches.md#stacking-a-branch-records-its-parent) has the rule.

**chain** — One worktree's own line of the [operation log](snapshots-and-undo.md): every operation belongs to the chain of the worktree that ran it, `ff undo` steps back the chain of the tree it runs in, and a chain outlives its worktree. The [worktrees guide](../guides/worktrees.md#one-repository-a-log-per-tree) shows the split.

**change** — The unit of work in progress: the working copy is the change, with no index or staging area in front of it. A change is in exactly one of three states — **open**, the working copy being edited right now, of which every worktree has exactly one; **parked**, set aside with a branch you switched away from; **closed**, a commit — and [changes](changes.md) walks the transitions.

**claim** — Give a branch a name you chose with [`ff describe -b <name>`](../reference/cli/describe.md), replacing its petname or an earlier name; there is no separate rename command. The rename carries everything fufu associates with the branch — the capture chain, any parked change, the pending description — as [branches](branches.md) describes.

**close** — Turn the [open change](changes.md) into a commit: [`ff commit`](../reference/cli/commit.md) is the verb, paths close a slice, and closing is the only way a change enters history. A closed change is an ordinary git commit.

## F–L

**the floor** — The operation log's first entry, taken when the repository was armed. [Undo](snapshots-and-undo.md) reaches back to the floor and no further: everything before fufu's arrival is git's history, not fufu's timeline.

**foreign operation** — An operation recording what raw git did behind fufu's back, absorbed lazily into the operation log at the next fufu invocation — labeled as foreign, quoted with git's own reflog messages, and undoable like anything fufu did itself. [The two regimes](two-regimes.md) covers the boundary it crosses.

**held rewrite** — A pending rewrite that stopped at a conflict: no ref moved, no half-applied tree touched the repository, and the verb's question — the branch, the target — is recorded for a moment you choose. A hold blocks [`ff publish`](push-boundary.md) and nothing local; [held rewrites](held-rewrites.md) is the full story, and [`ff resolve`](../reference/cli/resolve.md) is the way out.

**lease** — The guard every [publish](push-boundary.md) carries: the push goes through only if the shared copy still stands where you last saw it, and stops otherwise with nothing sent and nothing lost.

## M–P

**map** — What bare `ff` draws: recent work across every branch, parked changes included — where you left things. It shows only the commits that relate the branches shown and contracts the runs between them; [`ff map`](../reference/cli/map.md) is its spelled-out name.

**mint** — Create an anonymous branch: every [`ff start`](branches.md) mints a real branch under a reserved prefix with a generated petname, deferring only the christening. `-b` names the minted branch at birth instead.

**operation** — One entry on the operation log: a verb fufu ran, a capture, or a foreign operation absorbed from outside. Every operation records all refs plus the tree state, which is why [undo](snapshots-and-undo.md) restores both together.

**operation id** — An operation's address, spelled in the letters k–z and never in hex, so a letters-spelled id is always an operation and a hex one always a commit. `@` is the newest operation and takes git's first-parent suffixes — `@^`, `@~3` — as [snapshots and undo](snapshots-and-undo.md) explains.

**operation log** — The one log every mutation fufu performs lands on, captures and foreign operations included; [`ff op log`](../reference/cli/op-log.md) lists it, newest first. [Snapshots and undo](snapshots-and-undo.md) explains why there is one log and one address space rather than two.

**park** — Set the [open change](changes.md) aside with its branch on a switch: an ordinary stash entry labeled with the branch, which becomes the open change again — same files, same edits, same pending description — when you switch back. [Branches](branches.md) covers the mechanics.

**pending description** — The description the open change carries before it is ever a commit, set with [`ff describe -m`](changes.md); when the change closes, `ff commit` picks it up as the commit message. It parks and resumes with the change.

**petname** — The generated name of an anonymous branch, like `ff/hidden-wren`: a genuine ref under a reserved prefix that every GUI shows, every git command addresses, and no push refspec matches by accident. See [branches](branches.md).

**publish** — The outgoing half of [the push boundary](push-boundary.md): [`ff publish`](../reference/cli/publish.md) sends the branch to its one remote, under a lease, and never rides along as a default inside any other verb.

## R–T

**replay** — Recreate commits one by one onto a new base, in memory, landing only when the result is clean; the first step that conflicts stops the run and becomes a [held rewrite](held-rewrites.md). Sync, restack, and fufu's other rewrites all move history this way.

**restack** — Replay a branch's commits onto the base it sits on; [`ff restack`](../reference/cli/restack.md) is the verb, and `--onto` records a new base first, which is how a branch is re-aimed. It is the primitive under [sync](push-boundary.md)'s replay and the rest of the [rewrites](held-rewrites.md), and the branches stacked on the moved branch follow it through the [cascade](branches.md#stacking-a-branch-records-its-parent).

**run** — [Undo](snapshots-and-undo.md)'s unit: the longest stretch of adjacent captures carrying the same session, ending at the first operation that is not one, so forty captures of an editing session are one keystroke back. Only captures group — a verb's operation is always its own step.

**slice** — The part of the open change that path arguments close: `ff commit src/parser.rs -m "one fix"` lands that file and leaves everything else open. Selection at the moment of the close, not a staging area; see [changes](changes.md).

**snapshot** — The tree state an operation carries, stored in refs outside the visible graph so the commit history you and your teammates read is untouched. Not a second concept with its own log and ids — [snapshots and undo](snapshots-and-undo.md) explains why restore is uniform because of it.

**sync** — The incoming half of [the push boundary](push-boundary.md): [`ff sync`](../reference/cli/sync.md) fetches once and, for every local branch, takes in what arrived from the base beneath it and the shared copy of it, replaying the branch's commits onto the result. Nothing it does leaves the machine, and one `ff undo` takes the whole run back.

**trunk** — The repository's main line — what "main" is — which bare [`ff start`](branches.md) forks from and which is the default base a branch answers to. fufu resolves it once per repository: config (`fufu.trunk`) first, heuristics otherwise, and ambiguity is an error naming the candidates, never a guess.
