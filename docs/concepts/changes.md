# Working copy and commits

<a id="changes"></a>

**The working copy is the change.** Edit your files, then [`ff commit`](../reference/cli/commit.md) records the work in branch history. There is no staging step: the command takes the current files, or just the paths you name.

[`ff switch`](../reference/cli/switch.md) sets uncommitted work aside with the branch you leave and restores the work on the branch you select. You can move to another task without making a temporary commit yourself.

A change has three states:

- **Open** — the files you are editing in the current worktree. An open change can be empty: its files match the commit beneath it.
- **Parked** — uncommitted work set aside with a branch when you switch away. Switching back resumes it, including its pending commit message.
- **Closed** — work recorded in branch history by `ff commit`. This is what fufu means by *closing* a change.

[Snapshots](snapshots-and-undo.md) save intermediate file states for recovery. They do not add commits to your branch history.

## Closing is the commit

`ff commit -m "fix parser options"` records the open change and leaves you with an empty open change ready for more edits. A clean working copy has nothing to commit; the command does not create an empty commit. [`ff undo`](../reference/cli/undo.md) can take the commit back, restoring the recorded local refs and files together.

<a id="closing-a-slice"></a>

### Partial commits

To record only part of your work, give paths:

```sh
ff commit src/parser.rs -m "fix parser options"
```

This commits `src/parser.rs` and leaves the other edits open. A path can be a file or a directory, including its subtree; globs are not supported. The same path selection is available in [`ff restore`](../reference/cli/restore.md) and [`ff diff`](../reference/cli/diff.md).

The pending message goes with the part you commit. The remaining open change has no description until you give it one.

## Pending descriptions

[`ff describe -m "fix parser options"`](../reference/cli/describe.md) sets the open change's pending commit message. Bare `ff describe` opens `$EDITOR` with the current message. You can describe the work before making any edits.

The description stays with the change through edits and branch switches. `ff commit` uses it; `ff commit -m` supplies a replacement for that commit. After a partial commit, use `ff describe -m` to describe the remaining work.

### No index to keep in sync

fufu commits working-copy content without using Git's index as a selection step. Path arguments select files when you commit; they do not leave a staged selection behind. Hunk-level selection is not available through `ff commit`. See the [FAQ](../faq.md#can-i-commit-some-hunks-of-a-file-and-leave-the-rest) for that tradeoff and the Git commands available when you need it.

<a id="parking-travels-with-the-branch-forks-open-clean"></a>

## Parking and resuming

On `ff switch`, the open change stays with the branch you leave. The destination's parked change becomes your working copy, with the same edits and description. Both sides of the switch are reported.

If the destination's branch tip has moved, fufu replays its parked change onto the new tip. A conflict completes the branch switch but leaves the parked work held for [resolution](held-rewrites.md#parked-change-arrival). Git's staged selection does not travel with parked work: staged hunks return as unstaged edits.

### Forks open clean

[`ff start`](../reference/cli/switch.md) with no target creates a branch from trunk, the repository's main development branch, and opens an empty change there. `ff start -b hotfix` gives the new branch a name. An existing branch target resumes that branch; use `ff start main -b hotfix` to create a new branch from it.

`ff start @` is the special case for carrying work to a new branch: it creates the branch at the commit beneath the open change and copies that change, including its description. The original branch keeps its own copy. Other new-branch targets leave the work parked on the original branch. See [branches and parked work](branches.md) for naming and base selection.

## The `@` row

[`ff status`](../reference/cli/status.md) shows the open change as the `@` row above the branch's committed history, marked with `●`:

```console
$ ff status
on ff/hidden-wren · nothing to pull
@  ozqwnnpu 5186836d   1m ago
│  (no description)
│  M src/main.rs   +1  -0  +++++++
│  A src/parser.rs +3  -0  ++++++++++++++++++++
│    2 files       +4  -0
●  qotkrumr 8d58f6b9   3m ago
│  release: cut v0.1.0
```

The letters identify the change; the hexadecimal value identifies its current Git commit object. The file summary compares the open change with the commit beneath it. `no changes` means those files match, even if the open change has a pending description.

Bare `ff` shows the current open change and marks branches with parked work. [`ff log`](../reference/cli/log.md) also shows `@`; revision or path filters include it only if it matches the requested selection.

## Internal storage and branch history

fufu stores the open change as a Git commit object under `refs/fufu/open/<branch>`, updating it as snapshots or descriptions change. A parked change is that same kind of object, kept under its branch's name. `git log --all` can show these objects even though they have not been committed to branch history.

Describing work changes this internal object's message and hash. It does not advance the branch. Committing advances the branch, sometimes directly to that object. A different message, a partial commit, signing, or a hook that changes content or the message can require a different object. The hash shown on `@` therefore need not be the hash of the final commit.

## A change has an identity

A **change ID** follows work through fufu rewrites even when its commit hash changes. fufu assigns the open change an ID at its first snapshot or description; the same ID follows it into branch history. Rewording, restacking, and absorbing edits preserve the IDs of surviving changes.

Change IDs use the letters k–z. fufu stores the full ID, representing sixteen random bytes, in a `change-id` commit header that jj also understands. A commit without that header gets an ID derived from its hash, so rewriting it outside fufu can change that derived identity. The [Git compatibility page](two-regimes.md#lazy-absorption) explains what fufu can observe after outside changes.
