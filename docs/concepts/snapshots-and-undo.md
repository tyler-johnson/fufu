# Snapshots and undo

**Snapshots preserve the file state fufu has captured. Undo restores retained snapshots and recorded local operations.**

fufu calls each snapshot a **capture**. Capture is normally automatic when something invokes fufu:

- Repository readers attempt a capture before reading. Mutating verbs capture before their local changes, after initial guards; a pull's fetch and a command's automatic fetch can run before that capture. Help, version, watch, setup commands, and some dry runs do not take a pre-command snapshot.
- [`ff git`](../reference/cli/git.md) attempts a capture before running Git. A strict-policy refusal stops before that capture; a capture failure prints a warning and Git still runs.
- Installed and active agent hooks capture on the events they receive. Shell integration adds a `git='ff git'` alias and prompt captures. An editor, script, or shell that bypasses those integrations does not automatically invoke fufu.

[`ff trigger`](../reference/cli/trigger.md) takes a manual snapshot; `ff trigger -m "before refactor"` labels it. An unchanged tree produces no new capture. Reader and hook captures are best-effort and may fail or lose a lock race, so an invocation alone does not prove that a new recovery point exists.

Automatic captures describe what ran or which agent acted. A manual capture can carry your `-m` description. [`ff describe`](../reference/cli/describe.md) instead sets the [open change](changes.md)'s pending commit message, and [`ff commit`](../reference/cli/commit.md) records that change in branch history.

Captures live in refs outside the visible graph, so the commit history you and your teammates read is untouched. That is [the invariant](invariant.md) at work.

### Coverage and limits

Captures include tracked files and non-ignored untracked files. Ignored untracked files and unsaved editor buffers are outside the snapshot. `fufu.maxFileSize` defaults to 50 MiB: regular files above the limit are skipped when hashing working-copy content, including modified tracked files; content already in the index or base tree can still be present. A successful capture does not mean every file's latest content was saved.

Recovery reaches only states actually captured and still retained. `fufu.keep` defaults to 90 days, and automatic trimming runs daily by default. Trimming can remove recovery points and rewrite operation IDs. Uncaptured edits destroyed by raw Git or an editor cannot be reconstructed from ref history.

Undo follows the current worktree's operation chain. It restores that operation's local refs, HEAD, index, and files, subject to worktree guards; it does not roll back another worktree's chain. Remote pushes, other clones, CI, and hook or tool effects outside the recorded repository state are beyond its reach. See [worktrees](../guides/worktrees.md) and [the push boundary](push-boundary.md).

## One log, one address space

Every capture is an **operation**. A snapshot is not a second concept with its own log and its own ids — it is what an operation carries.

Captures and recorded local operations share one log per worktree. Entries carry file state and recorded ref state, so undo can restore both together. Network and maintenance effects do not all belong to that log.

Operations differ only in what they contain:

- **A capture** moves no ref. It is the tree alone, taken at machine rate.
- **A verb's operation** carries ref movements too — a switch, a commit, a pull.
- **A foreign operation** records what raw git did behind fufu's back, absorbed lazily at the next fufu invocation. [The two regimes](two-regimes.md) covers that boundary.

At the next reconciliation, ref changes made around fufu enter the log. Their recovery points are limited to the states fufu observed.

These kinds sort the log; they do not fork the model. Every operation has a tree, which is what makes restore uniform — the same thing happens whichever entry you name.

### Addressing an operation

[`ff op log`](../reference/cli/op-log.md) lists every operation, newest first, and every means every. Captures outnumber verb operations by more than ten to one, so the log is mostly a machine's account of itself.

Operation ids are hex, like commit ids and like jj's, and print at twelve characters in every column. The slot decides which space a hex prefix is read in: an operation slot — `ff op`, `ff history`, `--at-op` — reads an operation id, and a revision slot — `ff log -r`, `ff show`, `ff describe <rev>` — reads a sha or a [change id](changes.md#a-change-has-an-identity). Letters are a change id and nothing else. An id typed in the other kind of slot is refused by name, pointing at the verb that reads it.

`@` is the newest operation, and git's first-parent suffixes work on it — `@^` is the one before, `@~3` three back — because an operation's first parent is the operation before it.

The rest of the [`ff op`](../reference/cli/op.md) family reads and moves the log. `show` reads an operation and its ref transitions; `diff` compares the files in two operation trees. `restore` rewinds the current worktree's recorded state to one, and `revert` inverts one where later ref movements still allow it.

## Undo steps over runs

A capture is a machine's granularity, and a person's undo is not. Stepping back one operation at a time through forty captures of an editing session would make [`ff undo`](../reference/cli/undo.md) useless.

So undo steps over a **run**: the longest stretch of adjacent captures from the same session, ending at the first operation that is not one. Forty captures of the same stretch of work are one keystroke back.

Only captures group this way. A verb's operation is a decision somebody made, so it is always its own step — a switch and a commit are two undos, never one. That is also what keeps undo from rolling past a commit by accident, since [closing a change](changes.md) always ends a run.

Undo says what a run collapsed, because a keystroke that moved forty operations should not have to be inferred. The finer address survives untouched: [`ff op restore <op>`](../reference/cli/op-restore.md) still lands on any single operation, captures included.

## Undo moves a pointer, never appends

`ff undo` steps the log's pointer back to the run's predecessor. It does not write an entry saying that it did.

The log records work and never navigation, so undoing an undo is not something anyone has to reason about. Where the pointer has *been* is recorded where git already keeps such things, in the ref's own reflog.

Nothing is discarded. What an undo steps off stays reachable as a branch of the log, with the capture taken just before the undo at its head. [`ff redo`](../reference/cli/redo.md) walks forward along it, so the work you were holding when you undid is the first thing redo hands back.

Landing new work after an undo forks the log rather than truncating it. Redo stops offering a path it can no longer take, and says so, but the forked-off branch keeps its ids. `ff op restore` still lands on any of them until [`ff op trim`](../reference/cli/op-trim.md) ages them out.

## `ff history` is the keystroke map

`ff op log` answers what happened. [`ff history`](../reference/cli/history.md) answers where you can go back to. Those are different questions, because an honest log is mostly machine-rate rows.

One row is one keystroke. `@` is where the repository stands, each row below it is one more press of `ff undo`, and each row above is one more press of `ff redo`.

A run of captures collapses into the single row it undoes as, annotated with how many operations it collapsed. The rows above `@` are whatever is still reversible — once new work forks the log, they stop being offered.

The ids are the ones the `ff op` verbs take, so any row is also an [`ff op show <id>`](../reference/cli/op-show.md) and an `ff op restore <id>` target.

## The floor

Undo reaches back to the moment fufu started watching, and no further.

In a repository fufu did not create, the log's first entry is a floor operation:

```
operation log initialized from observed state; earlier operations not undoable
```

fufu builds its picture from what it observes, and everything before its arrival is git's history rather than fufu's timeline.

### Gaps of raw git motion

The same bound shapes what foreign work can offer. A gap of raw git motion collapses into a single foreign operation with restore points only at its endpoints.

The reason is that git's reflogs record where refs moved but never what the working copy held at each step. Expanding the gap would manufacture entries with nothing to restore. The *account* inside the operation can still be rich, quoting git's own reflog messages — explanation and restore have different granularities, and only the second is bounded by what git left behind.

Sharper still: a foreign tree change that moves no ref — a raw `git restore <file>`, an editor discarding a buffer — is invisible until the next capture, so it can destroy work fufu never saw.

How far back you can reach is set by successful captures and retention, not by when an edit or destructive command happened.
