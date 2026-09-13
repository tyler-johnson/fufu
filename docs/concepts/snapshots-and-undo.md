# Snapshots and undo

fufu takes snapshots when repository commands or active hooks invoke it. These save intermediate file states without adding commits to branch history. [`ff undo`](../reference/cli/undo.md) can restore a saved state and its recorded local refs, within the coverage and retention limits below.

## When snapshots run

- Repository readers attempt a snapshot before reading. Mutating commands take one before their local changes, after initial guards. A pull's fetch or a command's automatic fetch can run first. Help, version, watch, setup commands, and some dry runs do not take a pre-command snapshot.
- [`ff git`](../reference/cli/git.md) attempts a snapshot before running Git. A strict-policy refusal happens before that snapshot. A snapshot failure warns and still lets Git run.
- Installed and active agent hooks take snapshots on the events they receive. Shell integration provides a `git='ff git'` alias and prompt snapshots. An editor, script, or shell that bypasses those integrations does not invoke fufu automatically.

[`ff trigger -m "before refactor"`](../reference/cli/trigger.md) takes a manual snapshot with a label. An unchanged tree produces no new snapshot. Reader and hook snapshots are best-effort: failures or lock contention can prevent a new recovery point. fufu does not continuously watch files. See [hooks](../reference/hooks/index.md) to install and activate an integration.

## Coverage and limits

Snapshots include tracked files and non-ignored untracked files. Ignored untracked files and unsaved editor buffers are excluded. `fufu.maxFileSize` defaults to 50 MiB: regular files above the limit are skipped when hashing working-copy content, including modified tracked files. Content already in the index or base tree can still be present, so a successful snapshot does not mean every file's latest content was saved.

Recovery requires a successful snapshot that is still retained. `fufu.keep` defaults to 90 days, and automatic trimming runs daily by default. Trimming can remove recovery points and rewrite operation IDs. Uncaptured edits destroyed by Git or an editor cannot be reconstructed from ref history.

Undo follows the **current worktree's operation chain**. It restores recorded local refs, HEAD, index, and files, subject to worktree guards; it does not undo another worktree's chain. Remote updates, other clones, CI, and hook or tool effects outside the recorded repository state are beyond its reach. See [worktrees](../guides/worktrees.md) and [pulling and pushing](push-boundary.md).

## Choosing a recovery command

| What you want | Command | What it restores |
| --- | --- | --- |
| See available undo and redo steps | [`ff history`](../reference/cli/history.md) | Nothing; lists the recovery steps. |
| Go back one step | `ff undo` | Recorded refs and working-copy state together. |
| Go forward after undo | [`ff redo`](../reference/cli/redo.md) | The next state on the available redo path. |
| Discard edits to a file | [`ff restore path`](../reference/cli/restore.md) | The file from the commit beneath the open change. |
| Recover a file from a commit | `ff restore path --from <rev>` | Selected file content, leaving branch history in place. |
| Recover a file from a snapshot | `ff restore path --at-op <op>` | Selected file content from the named operation. |
| Restore a whole recorded state | [`ff op restore <op>`](../reference/cli/op-restore.md) | This worktree's recorded state at that operation, including refs. |

Use `ff history` first to find the step you need. The [recovery guide](../guides/recovery.md) shows these choices in complete examples.

<a id="ff-history-is-the-keystroke-map"></a>

## Reading `ff history`

Each row represents an undo step. `@` marks the current state; rows below it are successive undo targets, and rows above it are successive redo targets. A row can group several automatic snapshots and reports how many it includes.

The row's operation ID also works with [`ff op show`](../reference/cli/op-show.md), which shows the operation and its ref transitions. Inspect it before using `ff op restore` when you need one exact state.

## Undo steps over runs

Adjacent snapshots from the same session form a **run**. Undo treats that run as one step, so forty snapshots of an editing session do not require forty undo commands.

Only snapshots group this way. Each recorded command operation is its own step: a branch switch and a commit take two undos. A command operation also ends the adjacent snapshot run. For finer recovery, `ff op restore` accepts any retained operation, including an individual snapshot within a run.

## One log, one address space

An **operation** is an entry in the current worktree's operation log. Entries carry file state and recorded ref state. They include:

- **Captures** — the technical name used in output for snapshots of working-copy state, without a user branch update.
- **Command operations** — recorded local changes such as a switch, commit, or pull, including their ref movements.
- **Foreign operations** — records of ref changes observed after Git or another tool changed the repository. [Using fufu alongside Git](two-regimes.md#lazy-absorption) explains their limits.

Automatic snapshot descriptions identify what ran or which agent acted. A manual snapshot can use your `-m` label. [`ff describe`](../reference/cli/describe.md) instead sets the open change's pending commit message; [`ff commit`](../reference/cli/commit.md) records that work in branch history.

[`ff op log`](../reference/cli/op-log.md) lists individual operations, including snapshots that `ff history` groups into one row. Fetch and maintenance effects are not all part of these recorded local operations.

### Addressing an operation

Operation IDs are hexadecimal, displayed at twelve characters. Commit hashes are also hexadecimal; [change IDs](changes.md#a-change-has-an-identity) use k–z. The command argument or flag decides what kind of ID is expected. `--at-op` and the [`ff op`](../reference/cli/op.md) commands use operation IDs; revision arguments use commits, change IDs, or revision expressions.

In an operation argument, `@` means the current operation, `@^` its predecessor, and `@~3` three operations back. These count individual operations, including snapshots, rather than the grouped steps in `ff history`. The same spelling in a revision argument addresses commit history instead.

The [Revisions and IDs reference](../reference/revisions.md#operation-expressions) defines operation prefixes, ranges, filters, and the separate meaning of `@` in revision arguments. Its [past-state table](../reference/revisions.md#paths-sources-and-past-state-reads) lists which commands implement `--at-op` and `--at`.

[`ff op diff`](../reference/cli/op-diff.md) compares files in operation trees; use `ff op show` to inspect ref transitions. [`ff op revert`](../reference/cli/op-revert.md) inverts an operation where subsequent ref changes still permit it.

## Undo moves a pointer, never appends

Undo moves the operation log's current pointer back instead of appending an undo entry. The state it leaves remains reachable, with a snapshot taken before undo preserving the work you were holding. Redo follows that path forward again.

New work after undo forks the operation history. Redo stops offering the previous path, but its operation IDs remain available to `ff op restore` until retention removes them. [`ff op trim`](../reference/cli/op-trim.md) manages that retained history.

<a id="the-floor"></a>

## Earliest recovery point

Undo cannot reach before fufu's first observation. In an existing repository, the initial operation is called a *floor* in output:

```text
operation log initialized from observed state; earlier operations not undoable
```

Commits from before that point are still Git history. fufu does not gain a record of earlier uncommitted file states by being enabled later. Retention can further shorten the available operation history.

<a id="gaps-of-raw-git-motion"></a>

### Changes made between observations

Several outside ref changes can appear as one foreign operation, with recovery points limited to the observed endpoints. Git's reflog can explain intermediate ref movements but does not preserve each intermediate working copy. A file-only edit or discard may leave no ref change at all. The [compatibility page](two-regimes.md#lazy-absorption) explains how fufu reports these observations when you return.
