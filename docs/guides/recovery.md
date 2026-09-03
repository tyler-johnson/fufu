# Recovery

The cookbook for when something is already wrong. Every transcript below is real `ff` output, from a repository a few commits into the tutorial's story: a `parser-stream` branch carrying two commits on top of `main`.

The model that makes all of it work is one sentence: fufu snapshots the working tree before every action and records every mutation on one operation log, so recovery is never reconstruction — it is naming where to go back to. [Snapshots and undo](../concepts/snapshots-and-undo.md) is that model in full. Two verbs cover almost everything here, and telling them apart is the main skill: [`ff undo`](../reference/cli/undo.md) moves refs and the working tree together, one step at a time, while [`ff restore <path>`](../reference/cli/restore.md) writes only worktree files and leaves refs, HEAD, and the index exactly where they stand.

## "An agent ran `git reset --hard`"

The symptom: an agent, a script, or you in a hurry ran something destructive with raw git, and now the branch points somewhere earlier and the working tree has been rewritten to match.

```console
$ git reset --hard HEAD~2
HEAD is now at 441fd61 release: cut v0.1.0
```

The reset was never dangerous, because fufu snapshotted the tree before it ran. At the next fufu invocation the foreign ref motion is absorbed into the operation log as an operation of its own — [the two regimes](../concepts/two-regimes.md) covers that boundary — and one `ff undo` takes it back like anything fufu did itself:

```console
$ ff undo
ff: absorbed changes made outside fufu:
  refs/heads/parser-stream moved to 441fd61c (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at sunquylzttrv (commit on parser-stream: parser: drop whitespace from the stream)
  refs/heads/parser-stream → 57145cd3
  2 worktree file(s) restored
back: ff redo
```

Refs and files come back together in the same operation. This is the recovery to reach for whenever the damage is repo-wide and recent, whoever or whatever caused it: `ff undo`, repeated until you are back where you want to be.

## "I want one file back the way it was"

The symptom: one file went wrong — a bad edit, an overzealous refactor — and the rest of the tree is fine, so a repo-wide undo is the wrong tool.

Bare `ff restore <path>` is the everyday "discard my edits to this file": it brings the file back as it stands in the commit under the open change.

```console
$ ff restore src/parser.rs
restored from 57145cd3 (parser: drop whitespace from the stream)
  restored  src/parser.rs
undo: ff undo
```

`--from <rev>` names a different source — a branch, a sha, any revset naming one revision. Here, `src/main.rs` as `main` last shipped it:

```console
$ ff restore src/main.rs --from main
restored from 441fd61c (release: cut v0.1.0)
  restored  src/main.rs
undo: ff undo
```

Only the worktree is written; branches and HEAD do not move. And a restore takes its own capture first, mandatorily, so a restore that turns out wrong is undone by another restore or by `ff undo`. The same verb also reads from the operation log instead of history: `--at-op <id>` restores a path as one operation held it, and `--at <time>` takes `30m`, `2h`, `3d`, or a date.

## "I want the whole tree from twenty minutes ago"

The symptom: a stretch of work went sideways across many files, and you want the repository as it stood before it started — not one file, and not just the last operation.

[`ff history`](../reference/cli/history.md) is the map of where you can go back to. `@` is where the repository stands; each row below is one more press of `ff undo`. A run of captures collapses into the single row it undoes as, and says how many it collapsed:

```console
$ ff history
@   wztzuoop    0s ago  now   pre: ff status
↓1  sunquylz    0s ago  undo  commit on parser-stream: parser: drop whitespace from the stream · 4 captures
↓2  wyoyqltx    0s ago  undo  pre: ff commit -m parser: drop whitespace from the stream
↓3  ozrwxrpr    0s ago  undo  claim ff/glad-beacon as parser-stream
↓4  zkywuypw    0s ago  undo  commit on ff/glad-beacon: parser: skeleton and char stream
↓5  vllovznk    0s ago  undo  pre: ff commit -m parser: skeleton and char stream
↓6  kvnzxxtx    0s ago  undo  switch from main to ff/glad-beacon
↓7  skxnynpt    0s ago  undo  mint branch ff/glad-beacon at 441fd61c
↓8  muvyysop    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

The letters-spelled ids in these rows are operation ids — spelled in k–z and never in hex, so a letters id is always an operation and a hex id always a commit; [snapshots and undo](../concepts/snapshots-and-undo.md#one-log-one-address-space) owns the address space. Every row is also an address the [`ff op`](../reference/cli/op.md) family takes. [`ff op show`](../reference/cli/op-show.md) confirms a row is the one you mean before anything moves:

```console
$ ff op show sunquylz
sunquylzttrv  op  0s ago
  commit on parser-stream: parser: drop whitespace from the stream
  on        parser-stream
  base      1b12cedc
  refs/heads/parser-stream → 57145cd3
  (the worktree is unchanged across it)
```

[`ff op restore <id>`](../reference/cli/op-restore.md) then lands on it directly — the whole repository, refs and tree together — instead of pressing undo row by row:

```console
$ ff op restore sunquylz
undid: pre: ff status
  now at sunquylzttrv (commit on parser-stream: parser: drop whitespace from the stream)
  2 worktree file(s) restored
back: ff redo
```

When you want only the files from twenty minutes ago and the refs as they are, that is a restore instead: `ff restore --all --at 20m` writes the whole tree from the operation current at that time and moves nothing else.

## "I undid too far"

The symptom: you pressed `ff undo` once more than you meant to, and a commit you wanted is now open again.

```console
$ ff undo
undid: commit on parser-stream: parser: drop whitespace from the stream
  now at wyoyqltxllnm (pre: ff commit -m parser: drop whitespace from the stream)
  refs/heads/parser-stream → 1b12cedc
back: ff redo
```

Undo moves the log's pointer rather than discarding anything, so [`ff redo`](../reference/cli/redo.md) steps forward again, one run at a time, until the log is back where it started:

```console
$ ff redo
redid: commit on parser-stream: parser: drop whitespace from the stream
  now at sunquylzttrv (commit on parser-stream: parser: drop whitespace from the stream)
  refs/heads/parser-stream → 57145cd3
back: ff undo
```

Landing new work after an undo forks the log instead of destroying the path you stepped off. Suppose you undo the same commit again, but this time close it differently rather than redoing:

```console
$ ff undo
undid: commit on parser-stream: parser: drop whitespace from the stream
  now at wyoyqltxllnm (pre: ff commit -m parser: drop whitespace from the stream)
  refs/heads/parser-stream → 1b12cedc
back: ff redo

$ ff commit -m "parser: drop whitespace and comments"
closed 7ca71c5f on parser-stream: parser: drop whitespace and comments (1 file(s))
undo: ff undo

$ ff redo
ff: nothing to redo: work has landed since the last undo, so the log forked rather than rewound
  try:
    ff op log
    ff undo
```

Redo stops offering a way forward it can no longer take, and says so. Nothing was destroyed: the forked-off branch of the log keeps its ids, `ff op log` still lists them, and `ff op restore` still lands on any of them until [`ff trim`](../reference/cli/trim.md) ages them out.

## "Two writers on one chain, and only one was wrong"

The symptom: two agents share one worktree, so their operations land on one chain in turn. Agent A closed a commit that should not have happened, agent B has since started a branch off trunk and committed there, and `ff undo` would take back B's work first, because undo steps the chain from its newest operation whoever wrote it.

[`ff op log`](../reference/cli/op-log.md) is the chain; find the operation that was wrong:

```console
$ ff op log -n 6
onlwlmpy   0s ago  op      changelog     commit on changelog: changelog: start one
orpskpmt   0s ago  capture changelog     pre: ff commit -m changelog: start one
rpqkpopy   0s ago  op      changelog     switch from parser-stream to changelog
sptlwqxv   0s ago  op      parser-stream  mint branch changelog at 476c18f8
qnnvvtls   0s ago  op      parser-stream  commit on parser-stream: README: point at the parser
szovqqzy   0s ago  capture parser-stream  pre: ff commit -m README: point at the parser
```

[`ff op revert <op>`](../reference/cli/op-revert.md) inverts that one operation and leaves everything after it standing:

```console
$ ff op revert qnnvvtls
reverted qnnvvtlspsorxzooruqkylozzzokqyztnpytmzxn: commit on parser-stream: README: point at the parser
  refs/heads/parser-stream → aa9d3216
undo: ff undo
```

B's branch and B's commit are untouched, and the revert is itself an operation on the chain, so `ff undo` takes the revert back too. Revert inverts refs, and it applies only where the refs the wrong operation moved still stand where it left them: a later commit on the same branch moves that ref, so had B committed on parser-stream as well, the revert would hold and change nothing. That case is a rewrite rather than a recovery — [`ff lift --from <rev>`](../reference/cli/lift.md) takes A's files back out of the closed commit, and drops the commit when it takes everything; [rewriting history](rewriting-history.md#split-a-commit-that-already-closed) has it.

## "I committed to the wrong branch, or with the wrong message"

The symptom: the close itself was fine, but it landed with the wrong name on it — or on the wrong branch entirely.

A wrong message never needs the commit reopened. [`ff describe <rev>`](../reference/cli/describe.md) rewords a commit that has already closed, restacking anything above it in the same operation:

```console
$ ff commit -m wip
closed dc6b36dc on parser-stream: wip (1 file(s))
undo: ff undo

$ ff describe dc6b36dc -m "parser: string literals"
reworded ea9920b5 on parser-stream: parser: string literals
undo: ff undo
```

A commit on the wrong branch, if it just happened, is one `ff undo`: the close comes back open, tree and refs together, and you close it again where it belongs — [`ff commit -b <branch>`](../reference/cli/commit.md) lands the close on a fresh branch in the same step. Once later work has piled on top, this stops being a recovery problem and becomes a rewriting one: [`ff absorb --into <rev>`](../reference/cli/absorb.md) folds the open change into the commit it should have joined, `ff lift --from <rev>` takes files back out of a closed commit, and [`ff edit <rev>`](../reference/cli/edit.md) reopens one in place. [Rewriting history](rewriting-history.md) is the guide for that whole family.

## "Someone force-pushed over my branch"

The symptom: you go to publish and fufu refuses — the shared copy of your branch is not where you left it, because somebody rewrote it and force-pushed.

Every [`ff publish`](../reference/cli/publish.md) carries a lease: the push goes through only if the shared copy still stands where you last saw it. So the force-push cost you nothing — the lease caught it, nothing was sent, and nothing was lost:

```console
$ ff publish
ff: origin/parser-stream moved since you last looked, so nothing was pushed — your commits are still here, and ff sync takes in what arrived
  try:
    ff sync
    ff publish
```

[`ff sync`](../reference/cli/sync.md) asks whether what the shared copy holds beyond you is new work or old versions of yours. Here it is new work, so it is taken in and your commits replay on top; a commit of yours that the rewrite already contains replays empty and is dropped, and sync says which:

```console
$ ff sync
fetching from origin
took in 1 commit(s) from origin/parser-stream
replayed 1 of yours on top
dropped ea9920b5 "parser: string literals" — it changes nothing
1 commit(s) to publish — ff publish
undo: ff undo
```

Nothing has left the machine yet, and everything sync did is one `ff undo` away. Once the branch lines up, publish sends it, under a fresh lease:

```console
$ ff publish
published parser-stream to origin/parser-stream
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

The same rules protect the other side: if your own publish would have overwritten work somebody pushed in good faith, the lease refuses that too. [The push boundary](../concepts/push-boundary.md) covers leases, rollback, and `--dry-run`.

## What undo cannot reach

Two things sit outside the net, and fufu tells you about both at the moment they matter.

The first is the push. `ff undo` moves this repository, and a push moves a machine somewhere else — which is why publishing is a verb you type rather than a step riding inside another one, and why every publish says so as it goes:

```console
$ ff publish
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

There is still a way back, and it is another publish rather than an undo: undo the commit locally, publish again, and the lease rolls the shared copy back to where the branch now stands. That is not erasure — other clones may hold the commits, CI ran — but the shared copy is yours to move.

The second is the floor. Undo reaches back to the moment fufu started watching, and no further. In a repository fufu adopts rather than creates, the log's first entry says exactly that, and `ff history` marks it:

```console
$ ff init
already a git repository on main
the net is on: ff undo has a floor to land on, and every verb takes one first

$ ff history
@   wuowulov    0s ago  now   operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

Everything before fufu's arrival is git's history, not fufu's timeline: reachable with git's own tools, but not a place `ff undo` can land. The same bound has a sharper edge in day-to-day work — a foreign tree change that moves no ref, like a raw `git restore` or an editor discarding a buffer, is invisible until the next capture, so how far back you can reach is set by the last time fufu was looking. For everything done through fufu's own surface, that is always the moment before it happened. [Snapshots and undo](../concepts/snapshots-and-undo.md) closes the loop on why.
