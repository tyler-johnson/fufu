# Stacked changes

A stack is a branch whose base — the branch it forked from and rebases onto — is itself a branch under review. You split a feature into reviewable pieces, each piece on its own branch, each branch forked from the tip of the one below, and each published for its own review.

This guide builds a two-branch stack, lands review feedback at the bottom, lets the cascade carry the branch above, pulls the stack, and pushes each branch under its own lease — the guard that refuses a push when the shared copy has moved. The repository is the tutorial's demo, and every console block is real `ff` output.

The verbs already know the shape. [`ff start`](../reference/cli/start.md) records which branch a fork came from, every verb that moves a branch's tip replays the branches stacked on it onto the new tip, [`ff pull`](../reference/cli/pull.md) lines a branch up with its base and its remote, and [`ff push`](../reference/cli/push.md) sends the branch you stand on, or every branch you name. A stack is those verbs applied at the bottom, with the cascade doing the climbing.

## Start a stack

The bottom of the stack begins like any other [branch](../concepts/branches.md): `ff start` forks from trunk.

```console
$ ff start -b parser-core
minted parser-core (forked from main)
open change on parser-core
undo: ff undo
```

Three commits build the parser core — the skeleton, the wiring into `main`, and a buffered stream:

```console
$ ff commit -m "parser: skeleton and char stream"
closed 3e766b76 on parser-core: parser: skeleton and char stream (1 file(s))
undo: ff undo

$ ff commit -m "parser: wire the module into main"
closed 0fffcd68 on parser-core: parser: wire the module into main (1 file(s))
undo: ff undo

$ ff commit -m "parser: buffered char stream"
closed dbf80757 on parser-core: parser: buffered char stream (1 file(s))
undo: ff undo
```

The next piece — a CLI flag that exposes the parser — depends on all of that, and it should not wait for parser-core's review. Fork the second branch at parser-core's tip by naming it: a branch name given to `ff start` forks there rather than continuing it — continuing is [`ff switch`](../reference/cli/switch.md)'s job.

```console
$ ff start parser-core -b parser-cli
minted parser-cli (forked from parser-core)
open change on parser-cli
undo: ff undo

$ ff commit -m "cli: expose the parser behind a flag"
closed 527ad478 on parser-cli: cli: expose the parser behind a flag (1 file(s))
undo: ff undo
```

`forked from parser-core` is the load-bearing line. The fork recorded parser-core as parser-cli's parent, and that record is what aims every later replay: nothing ever has to be told where parser-cli belongs.

## Read the stack in the map

Bare `ff` is the map, and a healthy stack reads as one column: branch markers stacked over each other, each branch's commits sitting directly on the tip of the branch below.

```console
$ ff
@  no changes                  ▸ [parser-cli]
│  (no description)
●  ypmunmok 527ad478   0s ago
│  cli: expose the parser behind a flag
●  suszrkom dbf80757   0s ago  ▸ [parser-core]
│  parser: buffered char stream
~  2 commits
●  ssrnpyvm d41ac877   0s ago  ▸ [main]
│  release: cut v0.1.0
●  mmpmvmzw 30d4dadd   0s ago
   init: hello world
```

`@` is the open change on parser-cli, `▸ [parser-core]` stands mid-column because parser-cli's commit sits directly on its tip, and a `~` row elides commits the map does not need to show.

A stack in good shape has no forks in this picture. Every rewrite below carries the branches above with it, so a fork here is the map telling you a branch was left behind, and the verb that left it said so at the time.

## Land review feedback with absorb

Review feedback arrives on the bottom branch: the wiring commit should say how the parser is reached. Switch to parser-core, make the edit, and fold it into the commit it belongs to with [`ff absorb`](../reference/cli/absorb.md).

```console
$ ff switch parser-core
switched to parser-core
undo: ff undo

$ ff absorb --into 0fffcd68
absorbed into 46f6832e: parser: wire the module into main
restacked 1 commit(s) above it
parser-cli followed parser-core: replayed 1 commit(s)
undo: ff undo
```

`restacked 1 commit(s) above it`: everything above the target on this branch re-parented in the same operation, with no interactive rebase and no fixup commit. `parser-cli followed parser-core: replayed 1 commit(s)` is the cascade. parser-cli's recorded base is parser-core, parser-core's tip moved, so parser-cli's one commit was replayed onto the new tip inside the same operation, and you never left parser-core. The map is still one column:

```console
$ ff
@  no changes                  ▸ [parser-core]
│  (no description)
│ ●  yvlxqopm dab92e41   0s ago  ▸ [parser-cli]
├─╯  cli: expose the parser behind a flag
●  txrmosvs 747bd826   0s ago
│  parser: buffered char stream
~  2 commits
●  ssrnpyvm d41ac877   0s ago  ▸ [main]
│  release: cut v0.1.0
●  mmpmvmzw 30d4dadd   0s ago
   init: hello world
```

`@` and `▸ [parser-cli]` both sit on parser-core's tip: the open change here and the branch above share a base, and the join is the map drawing two heads on one commit, not a fork.

## The cascade

Every verb that moves a branch's tip does what absorb just did. `ff restack`, `ff pull`, `ff absorb`, [`ff lift`](../reference/cli/lift.md), [`ff describe <rev>`](../reference/cli/describe.md), and [`ff done`](../reference/cli/done.md) each replay every local branch whose base is the branch they moved onto its new tip, parent before child, through the whole tree. The replays ride the verb's one operation, so one [`ff undo`](../reference/cli/undo.md) takes the rewrite and the cascade back together.

Each replay is performed rather than predicted: a branch above whose replay conflicts holds where it stands with nothing written there, the branches above it stay put because their base did not move, and the verb says so.

`ff switch` to the held branch and [`ff resolve`](../reference/cli/resolve.md) picks the replay up; when `ff done` lands it, the branches above it resume from there. [Held rewrites](../concepts/held-rewrites.md) covers that state.

Two kinds of branch are left where they stand and named: one checked out in another worktree, because only that worktree may move its HEAD, and one already holding a rewrite. [`ff restack <branch>`](../reference/cli/restack.md) is the verb for either once it is free: it replays the branch you name onto its recorded parent without touching a file on disk, and cascades above it the same way.

## Pull the stack

Meanwhile a teammate landed a commit on `main`. `ff pull` fetches once and lines the branch you stand on up with both things it answers to, the base beneath it and the remote copy of itself, cascading as it goes. The base comes level with its own remote copy first, and the branch above follows in the replay; `ff pull --all` is every local branch.

```console
$ ff pull
fetching from origin
main moved ahead by 1 commit(s)
replayed 3 commit(s) onto main
parser-cli followed parser-core: replayed 1 commit(s)
updated the working copy (1 file(s))
not published yet — ff push
main
    fast-forwarded to origin/main (1 commit(s))
undo: ff undo
```

Read it top down. parser-core, the branch you stand on, replayed onto the `main` that arrived; parser-cli followed it in the same replay; the working copy moved with parser-core. Then one block per other branch that did something: `main` fast-forwarded to what the teammate pushed. The whole run is one operation, offline, and one `ff undo` away.

## Push each branch under its own lease

Each branch in the stack goes to the remote as its own branch, so each piece gets its own review. `ff push` sends the branch you stand on; name every branch of the stack to send the stack, from wherever you stand, and each goes out under its own lease — it goes through only if that branch's shared copy still stands where you last saw it. Nothing comes along with a named branch, so a push of parser-cli alone leaves parser-core where it is.

```console
$ ff push parser-core parser-cli
created origin/parser-core and set parser-core to track it
parser-cli
    created origin/parser-cli and set parser-cli to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

The branch you stand on reads first, and every other branch in the run is a block headed by its name. The leases are per branch because the shared copies are: a teammate pushing to parser-core cannot make pushing parser-cli lie, and a refused lease on one branch costs nothing anywhere else — the rest of the run still goes out, that branch's block says what the wire said, and `ff pull parser-core` takes their work in and cascades, then push again. [The push boundary](../concepts/push-boundary.md) covers what the lease guards.

The finished stack, in the map:

```console
$ ff
@  no changes                  ▸ [parser-cli]
│  (no description)
●  ozlpwrtz 0683ae0b   1s ago
│  cli: expose the parser behind a flag
●  olnytotr 86b61ceb   1s ago  ▸ [parser-core]
│  parser: buffered char stream
~  2 commits
●  pmlqlyxr 821e9eda   1s ago  ▸ [main]
│  docs: say what this is
●  ssrnpyvm d41ac877   1s ago
│  release: cut v0.1.0
●  mmpmvmzw 30d4dadd   1s ago
   init: hello world
```

## When the bottom lands

Once parser-core merges into `main`, the top of the stack answers to trunk. `ff restack parser-cli --onto main` records `main` as its new parent and replays onto it — re-aiming is the same verb, with the parent said out loud once.

From here:

- [Rewriting history](rewriting-history.md) — absorb's whole family: edit, reword, split, and where restacking happens by itself.
- [Held rewrites](../concepts/held-rewrites.md) — what a conflicted cascade looks like and how `ff resolve` finishes it.
- [The push boundary](../concepts/push-boundary.md) — leases, rollback, and `ff push --dry-run`.
