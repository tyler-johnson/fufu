# Changes

**The working tree is the change.**

There is no object you assemble before committing — no index, no staging area, no draft. The edits sitting in your working tree are the change, from the first keystroke, and fufu [captures them automatically](snapshots-and-undo.md) as you work. Every verb that talks about work in progress talks about this one thing.

A change is in exactly one of three states:

- **Open** — the working tree, being edited right now. Every worktree has exactly one open change; when the tree matches the commit beneath it, the open change is empty, not absent.
- **Parked** — set aside with a branch when you switched away. A parked change is the open change that branch had, held as you left it, and it becomes the open change again when you switch back.
- **Closed** — a commit. Closing is how a change enters history, and [`ff commit`](../reference/cli/commit.md) is the verb that does it.

The verbs move a change between these states and do nothing else: `ff commit` closes, [`ff switch`](../reference/cli/switch.md) parks one change and reopens another, [`ff start`](../reference/cli/start.md) opens a fresh one. The rest of this page walks each transition.

## Closing is the commit

`ff commit` closes the open change into a commit. There is no `add` first, because there is nothing to add to: the tree is already the change. `-m` describes what is closing.

Path arguments close a slice. `ff commit src/parser.rs -m "one fix"` lands that file and leaves everything else open — still the change you are in the middle of. Paths follow the same rule `ff restore` and `ff diff` speak: a file, or a directory whose whole subtree lands, and no globs.

A slice is selection at the moment of the close, not a staging area. In git you maintain the index as a state between commits, keeping it in sync with what you intend; in fufu the selection is an argument to one command, made once, and nothing persists afterward. There is still nothing between commits. The part left open keeps no description — the one it had went out with the slice — and `ff describe -m` gives the remainder its own.

A clean tree has nothing to close, so `ff commit` on one is a no-op rather than an empty commit. And every close is a recorded operation: `ff undo` takes it back, tree and refs together.

## Pending descriptions

The open change carries a description before it is ever a commit. [`ff describe -m`](../reference/cli/describe.md) sets it inline; bare `ff describe` opens `$EDITOR` seeded with the current text. When the change closes, `ff commit` picks the pending description up as the commit message; `ff commit -m` wins over it.

This means you can name work while you are doing it, when the intent is freshest, instead of reconstructing it at the end. The description belongs to the change, not to the moment of committing: it shows in the `@` row, it parks and resumes with the change on `ff switch`, and it waits through however many edits come before the close.

A description does not create a commit. Describing a clean tree is legal — the text simply waits for the next close.

## Parking travels with the branch; forks open clean

`ff switch` moves between branches without a stash dance. Whatever is open is parked with the branch you are leaving; whatever was parked where you are going becomes the open change again — same files, same edits, same pending description. Both halves are reported, so you always know where your work went and what came back. Under the hood a parked change is an ordinary stash entry labeled with its branch, visible in any git GUI — [the invariant](invariant.md) at work; [branches](branches.md) covers the mechanics.

`ff start` is the other verb that leaves the open change behind. It forks a fresh branch — from trunk by default, or from a revision you name — and the change it opens there is clean and empty. Nothing ever crosses a fork: the open change parks where it was, on the branch it belongs to, and the new line of work begins from a commit alone. If the fork itself is the idea — you thought of the next task mid-edit — `ff start -m "the next thing"` opens the new change already described. `ff start` never creates a commit.

The three verbs divide the ground cleanly: `ff commit` records, `ff switch` resumes, `ff start` begins.

## The `@` row

Everywhere fufu draws the graph, the open change is the row marked `@`, sitting atop the commit walk of `●` rows. It is one notation across three views:

```console
$ ff status
on ff/hidden-wren · nothing to sync
@  ozqwnnpu 5186836d   1m ago
│  (no description)
│  M src/main.rs   +1  -0  +++++++
│  A src/parser.rs +3  -0  ++++++++++++++++++++
│    2 files       +4  -0
●  —        8d58f6b9   3m ago
│  release: cut v0.1.0
```

Bare `ff` — the map — shows the `@` row where you stand and marks other branches that hold a parked change. `ff status` reads the `@` row as a diffstat: the files that differ from the commit beneath it. `ff log` puts `@` atop the walk; with `-r` the row appears only when the open change is a member of the revset, and with paths only when the change touches them, because those are questions about a set, not about you.

The `@` row always exists. `no changes` means the tree matches the commit beneath it — the open change is empty, not gone — and the pending description prints under the row, so a change you have named but not yet closed already reads the way its commit will.
