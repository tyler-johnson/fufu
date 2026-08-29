# Snapshots and undo

**The working tree is snapshotted before every action, so no action can lose file state.**

Capture is entirely automatic, and there is no verb for asking. fufu snapshots the tree before every command it runs — before a switch, before a sync, before `ff git` hands your arguments to git — and around every mutation an agent or editor makes through it, at machine rate. The manual checkpoint is one of the rituals fufu exists to delete, the way the stash dance is: what you would reach for by hand already happened before the command you typed. A capture's description is written by fufu, never by a person — what ran, or which agent acted — so the record stays a machine's account of what happened rather than a place to leave notes. The channels for saying what work *means* are elsewhere: `ff describe` names the [open change](changes.md), `ff commit` closes it.

Snapshots live in refs outside the visible graph, so the commit history you and your teammates read is untouched — [the invariant](invariant.md) at work. And total capture is what licenses everything above it: automatic rebases, malleable unpublished commits, and agents editing at full speed are only defensible once no state can exist solely in the filesystem.

## One log, one address space

Every capture is an operation. A snapshot is not a second concept with its own log and its own ids — it is what an operation carries. Every mutation fufu performs lands on one operation log, and each entry records all refs plus the tree state, which is why undo restores both together and why there is one address space to learn rather than two.

Operations differ only in what they contain. A capture moves no ref — it is the tree alone, taken at machine rate. A verb's operation carries ref transitions too: a switch, a commit, a sync. A foreign operation records what raw git did behind fufu's back, absorbed lazily at the next fufu invocation — [the two regimes](two-regimes.md) covers that boundary — so by the time you ask, work done around fufu is in the log and undoable like anything fufu did itself. Kinds sort the log; they do not fork the model. Every operation has a tree, and that is what makes restore uniform: the same thing happens whichever entry you name.

[`ff op log`](../reference/cli/op-log.md) lists every operation, newest first, and every means every: captures outnumber verb operations by more than an order of magnitude, so the log is mostly a machine's account of itself. Operation ids are spelled in the letters k–z and never in hex, which is what keeps hex meaning "commit" everywhere in fufu — a letters-spelled id is always an operation, a hex one always a commit. `@` is the newest operation, and git's first-parent suffixes work on it — `@^` is the one before, `@~3` three back — because an operation's first parent is the operation before it. The rest of the [`ff op`](../reference/cli/op.md) family reads and moves the log: `show` and `diff` read one, `restore` rewinds the whole repository to one, `revert` inverts one leaving later work standing.

## Undo steps over runs

A capture is a machine's granularity and a person's undo is not. Stepping back one operation at a time through forty captures of an editing session would make [`ff undo`](../reference/cli/undo.md) useless, so undo steps over a *run*: the longest stretch of adjacent captures carrying the same session, ending at the first operation that is not one. Forty captures of the same stretch of work are one keystroke back.

Only captures group. A verb's operation is a decision somebody made, so it is always its own step — a switch and a commit are two undos, never one. This is also what keeps undo from rolling past a commit by accident: [closing a change](changes.md) always ends a run, so no amount of capture noise around it can fold the commit into a larger step. What a run collapsed, undo says, because a keystroke that moved forty operations should not have to be inferred. The finer address survives untouched — [`ff op restore <op>`](../reference/cli/op-restore.md) still lands on any single operation, captures included.

## Undo moves a pointer, never appends

`ff undo` steps the log's pointer back to the run's predecessor rather than writing an entry saying that it did. The log records work and never navigation, so undoing an undo is not something anyone has to reason about; where the pointer has *been* is recorded where git already keeps such things, in the ref's own reflog.

Nothing is discarded. What an undo steps off stays reachable as a branch of the log, with the capture taken just before the undo at its head — so [`ff redo`](../reference/cli/redo.md) walks forward along it, and the work you were holding when you undid is the first thing redo hands back. Landing new work after an undo forks the log rather than truncating it: redo stops offering a path it can no longer take, and says so, but the forked-off branch keeps its ids and `ff op restore` still lands on any of them until `ff trim` ages them out.

## `ff history` is the keystroke map

`ff op log` answers what happened. [`ff history`](../reference/cli/history.md) answers the other question — where you can go back to — and they are not the same question, because an honest log is mostly machine-rate rows.

One row is one keystroke. `@` is where the repository stands; each row below it is one more press of `ff undo`, and each row above is one more press of `ff redo`. A run of captures collapses into the single row it undoes as, annotated with how many operations it collapsed. The rows above `@` are whatever is still reversible: once new work forks the log, they simply stop being offered. And the ids are the ones the `ff op` verbs take, so any row is also an `ff op show <id>` and an `ff op restore <id>` target.

## The floor

Undo reaches back to the moment fufu started watching, and no further. In a repository fufu did not create, the log's first entry is a floor operation reading `operation log initialized from observed state; earlier operations not undoable`: fufu builds its picture from what it observes, and everything before its arrival is git's history, not fufu's timeline.

The same bound shapes what foreign work can offer. A gap of raw git motion collapses into a single foreign operation with restore points only at its endpoints, because git's reflogs record where refs moved but never what the working tree held at each step — expanding the gap would manufacture entries with nothing to restore. The account *inside* the operation can still be rich, quoting git's own reflog messages, but explanation and restore have different granularities, and only the second is bounded by what git left behind. Sharper still: a foreign tree change that moves no ref — a raw `git restore <file>`, an editor discarding a buffer — is invisible until the next capture, so it can destroy work fufu never saw. How far back you can reach is set by the last time fufu was looking, which, for everything done through the surface, is always the moment before it happened.
