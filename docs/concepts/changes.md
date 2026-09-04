# Changes

**The working tree is the change.**

There is no object you assemble before committing — no index, no staging area, no draft. The edits sitting in your working tree are the change, from the first keystroke.

fufu saves them for you as you work: every command takes a [capture](snapshots-and-undo.md) — an automatic snapshot of the tree — before it acts. Every verb that talks about work in progress is talking about this one thing.

A change is in exactly one of three states:

- **Open** — the working tree, being edited right now. Every worktree has exactly one open change. When the tree matches the commit beneath it, the open change is empty, not absent.
- **Parked** — set aside with a branch when you switched away. A parked change is the open change that branch had, held as you left it, and it becomes the open change again when you switch back.
- **Closed** — a commit. Closing is how a change enters history, and [`ff commit`](../reference/cli/commit.md) is the verb that does it.

The verbs move a change between these states and do nothing else. `ff commit` closes, [`ff switch`](../reference/cli/switch.md) parks one change and reopens another, [`ff start`](../reference/cli/start.md) opens a fresh one. The rest of this page walks each transition.

## Closing is the commit

`ff commit` closes the open change into a commit. There is no `add` first, because there is nothing to add to — the tree is already the change. `-m` describes what is closing.

Path arguments close a slice. `ff commit src/parser.rs -m "one fix"` lands that file and leaves everything else open, still the change you are in the middle of. Paths follow the same rule [`ff restore`](../reference/cli/restore.md) and [`ff diff`](../reference/cli/diff.md) speak: a file, or a directory whose whole subtree lands, and no globs.

A slice is selection at the moment of the close, not a staging area. git's index is a real capability — a place to assemble a commit hunk by hunk — but it costs you a third state to keep in sync between commits.

fufu trades the hunk-level assembly away and gets back having nothing to maintain. The selection is an argument to one command, path-level, made once, and nothing persists afterward. If `git add -p` is your daily habit, the [FAQ](../faq.md#can-i-commit-some-hunks-of-a-file-and-leave-the-rest) has the honest accounting and the escape hatch.

The part left open keeps no description, since the one it had went out with the slice. `ff describe -m` gives the remainder its own.

A clean tree has nothing to close, so `ff commit` on one does nothing rather than making an empty commit. Every close is a recorded operation, and [`ff undo`](../reference/cli/undo.md) takes it back — tree and refs together.

## Pending descriptions

The open change carries a description before it is ever a commit. [`ff describe -m`](../reference/cli/describe.md) sets it inline; bare `ff describe` opens `$EDITOR` seeded with the current text. When the change closes, `ff commit` picks that description up as the commit message, and `ff commit -m` wins over it.

So you can name work while you are doing it, when the intent is freshest, instead of reconstructing it at the end.

The description belongs to the change rather than to the moment of committing. It shows in the `@` row, it parks and resumes with the change on `ff switch`, and it waits through however many edits come before the close.

Describing does not create a commit. Describing a clean tree is legal — the text simply waits for the next close.

## Parking travels with the branch; forks open clean

`ff switch` moves between branches without a stash dance. Whatever is open is parked with the branch you are leaving. Whatever was parked where you are going becomes the open change again — same files, same edits, same pending description.

Both halves are reported, so you always know where your work went and what came back. Underneath, a parked change is an ordinary stash entry labeled with its branch, visible in any git GUI. That is [the invariant](invariant.md) at work; [branches](branches.md) covers the mechanics.

`ff start` is the other verb that leaves the open change behind. It forks a fresh branch — from trunk, your main line of development, unless you name a revision — and the change it opens there is clean and empty.

Nothing ever crosses a fork. The open change parks where it was, on the branch it belongs to, and the new line of work begins from a commit alone. If the fork itself is the idea, and you thought of the next task mid-edit, `ff start -m "the next thing"` opens the new change already described. `ff start` never creates a commit.

The three verbs divide the ground cleanly: `ff commit` records, `ff switch` resumes, `ff start` begins.

## The `@` row

Everywhere fufu draws the graph, the open change is the row marked `@`, sitting atop the commit walk of `●` rows. One notation, three views:

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

Bare `ff` — the map — shows the `@` row where you stand, and marks other branches holding a parked change. [`ff status`](../reference/cli/status.md) reads the `@` row as a diffstat: the files that differ from the commit beneath it.

[`ff log`](../reference/cli/log.md) puts `@` atop the walk. With `-r` the row appears only when the open change belongs to the range you asked for, and with paths only when the change touches them. Those are questions about a set of commits, not about you.

The `@` row always exists. `no changes` means the tree matches the commit beneath it, so the open change is empty rather than gone. The pending description prints under the row, so a change you have named but not closed already reads the way its commit will.
