# Stacked changes

A stack is a branch whose base is itself a branch under review. You split a feature into reviewable pieces, each piece on its own branch, each branch forked from the tip of the one below, and each published for its own review. This guide builds a two-branch stack, lands review feedback at the bottom, cascades the branch above, and publishes each branch under its own lease. The repository is the tutorial's demo, and every console block is real `ff` output.

The verbs already know the shape. [`ff start`](../reference/cli/start.md) records which branch a fork came from, [`ff restack`](../reference/cli/restack.md) replays a branch onto that recorded parent, and [`ff sync`](../reference/cli/sync.md) and [`ff publish`](../reference/cli/publish.md) line up and send the branch you stand on. A stack is those verbs applied bottom to top, one branch at a time.

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
closed 85ad0404 on parser-core: parser: skeleton and char stream (1 file(s))
undo: ff undo

$ ff commit -m "parser: wire the module into main"
closed 2a226695 on parser-core: parser: wire the module into main (1 file(s))
undo: ff undo

$ ff commit -m "parser: buffered char stream"
closed a72a7790 on parser-core: parser: buffered char stream (1 file(s))
undo: ff undo
```

The next piece — a CLI flag that exposes the parser — depends on all of that, and it should not wait for parser-core's review. Fork the second branch at parser-core's tip by naming it: a branch name given to `ff start` forks there rather than continuing it — continuing is `ff switch`'s job.

```console
$ ff start parser-core -b parser-cli
minted parser-cli (forked from parser-core)
open change on parser-cli
undo: ff undo

$ ff commit -m "cli: expose the parser behind a flag"
closed 50b3e7e5 on parser-cli: cli: expose the parser behind a flag (1 file(s))
undo: ff undo
```

`forked from parser-core` is the load-bearing line. The fork recorded parser-core as parser-cli's parent, and that record is what aims every later replay — restack never has to be told where the branch belongs.

## Read the stack in the map

Bare `ff` is the map, and a healthy stack reads as one column: branch markers stacked over each other, each branch's commits sitting directly on the tip of the branch below.

```console
$ ff
@  no changes                  ▸ [parser-cli]
│  (no description)
●  xurpmoqk 50b3e7e5   0s ago
│  cli: expose the parser behind a flag
●  —        a72a7790   0s ago  ▸ [parser-core]
│  parser: buffered char stream
~  2 commits
●  —        065d541c   0s ago  ▸ [main]
│  release: cut v0.1.0
●  —        9a9ebdba   0s ago
   init: hello world
```

`@` is the open change on parser-cli, `▸ [parser-core]` stands mid-column because parser-cli's commit sits directly on its tip, and a `~` row elides commits the map does not need to show. A stack in good shape has no forks in this picture; a fork is the map telling you a cascade is owed, which is exactly what the next two sections produce and repair.

## Land review feedback with absorb

Review feedback arrives on the bottom branch: the wiring commit should say how the parser is reached. Switch to parser-core — the branch above stays where it is — make the edit, and fold it into the commit it belongs to with [`ff absorb`](../reference/cli/absorb.md).

```console
$ ff switch parser-core
switched to parser-core
undo: ff undo

$ ff absorb --into 2a226695
absorbed into ad354cbd: parser: wire the module into main
restacked 1 commit(s) above it
undo: ff undo
```

`restacked 1 commit(s) above it`: everything above the target on this branch re-parented in the same operation, so parser-core is already whole — no interactive rebase, no fixup commit. The absorb stops at the branch's own tip. parser-cli still sits on the commits parser-core no longer has, and the map shows the fork:

```console
$ ff
@  no changes                  ▸ [parser-core]
│  (no description)
│ ●  —        50b3e7e5   0s ago  ▸ [parser-cli]
│ │  cli: expose the parser behind a flag
● │  —        3a560241   0s ago
│ │  parser: buffered char stream
● │  —        ad354cbd   0s ago
│ │  parser: wire the module into main
│ ~  2 commits
├─╯
●  xzossrtl 85ad0404   0s ago
│  parser: skeleton and char stream
●  —        065d541c   0s ago  ▸ [main]
│  release: cut v0.1.0
●  —        9a9ebdba   0s ago
   init: hello world
```

## Cascade one branch at a time

`ff restack <branch>` reaches a branch you are not standing on. The positional names the branch to move, the replay goes onto its recorded parent, and because you are not on parser-cli, no file on disk is touched — refs and objects only.

```console
$ ff restack parser-cli
parser-core moved ahead by 2 commit(s)
replayed 1 commit(s) onto parser-core
dropped 2 commit(s) that change nothing: 2a226695, a72a7790
undo: ff undo
```

The dropped lines are the cascade working. The replay walks from where parser-cli diverged, so it carries copies of parser-core's pre-absorb commits; each one replays to nothing on top of the rewritten branch and is dropped as changing nothing. What survives is parser-cli's own commit, now on parser-core's new tip, and the map is one column again:

```console
$ ff
@  no changes                  ▸ [parser-core]
│  (no description)
│ ●  —        7650fe23   0s ago  ▸ [parser-cli]
├─╯  cli: expose the parser behind a flag
●  —        3a560241   0s ago
│  parser: buffered char stream
~  2 commits
●  —        065d541c   0s ago  ▸ [main]
│  release: cut v0.1.0
~
```

A replay that would conflict does none of this: it stops with nothing written and holds, and `ff resolve` picks up every conflicted commit at once. [Held rewrites](../concepts/held-rewrites.md) covers that state.

## Sync the bottom, then cascade again

Meanwhile a teammate landed a commit on `main`. `ff sync` lines up the branch you stand on with both things it answers to — the base beneath it and the remote copy of itself. It moves that one branch only; cascading up a stack is one branch at a time, so the same `ff restack` follows it.

```console
$ ff sync
fetching from origin
main moved ahead by 1 commit(s)
replayed 3 commit(s) onto main
updated the working tree (1 file(s))
not published yet — ff publish
undo: ff undo

$ ff restack parser-cli
parser-core moved ahead by 4 commit(s)
replayed 1 commit(s) onto parser-core
dropped 3 commit(s) that change nothing: 85ad0404, ad354cbd, 3a560241
undo: ff undo
```

The rhythm is the whole discipline of a stack: move the branch you stand on with `sync` or `absorb`, then `ff restack <branch>` for each branch above it, in order. The sequence is deliberate — each move changes the base the next branch answers to, so each verdict is computed fresh rather than promised for the whole tree, and everything up to here is offline and one `ff undo` away.

## Publish each branch under its own lease

Each branch in the stack goes to the remote as its own branch, so each piece gets its own review. `ff publish` sends the branch you stand on, and every push carries its own lease — it goes through only if that branch's shared copy still stands where you last saw it.

```console
$ ff publish
created origin/parser-core and set parser-core to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease

$ ff switch parser-cli
switched to parser-cli
undo: ff undo

$ ff publish
created origin/parser-cli and set parser-cli to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

The leases are per branch because the shared copies are: a teammate pushing to parser-core cannot make publishing parser-cli lie, and a refused lease on one branch costs nothing anywhere else — `ff sync` on that branch takes their work in, cascade, publish again. [The push boundary](../concepts/push-boundary.md) covers what the lease guards.

The finished stack, in the map:

```console
$ ff
@  no changes                  ▸ [parser-cli]
│  (no description)
●  —        c95b1b83   0s ago
│  cli: expose the parser behind a flag
●  —        85e86fa0   0s ago  ▸ [parser-core]
│  parser: buffered char stream
~  3 commits
●  —        065d541c   0s ago  ▸ [main]
│  release: cut v0.1.0
~
```

## When the bottom lands

Once parser-core merges into `main`, the top of the stack answers to trunk. `ff restack parser-cli --onto main` records `main` as its new parent and replays onto it — re-aiming is the same verb, with the parent said out loud once.

From here:

- [Rewriting history](rewriting-history.md) — absorb's whole family: edit, reword, split, and where restacking happens by itself.
- [Held rewrites](../concepts/held-rewrites.md) — what a conflicted cascade looks like and how `ff resolve` finishes it.
- [The push boundary](../concepts/push-boundary.md) — leases, rollback, and `ff publish --dry-run`.
