# Recovery

The cookbook for when something is already wrong. Every transcript below is real `ff` output, from a repository a few commits into the tutorial's story: a `parser-stream` branch carrying two commits on top of `main`.

Recovery uses snapshots and local operations already recorded on this worktree's chain. Repository commands and active hooks take captures; they do not continuously watch files. Recovery depends on capture success and retention, and excludes uncaptured edits, ignored untracked files, unsaved buffers, and oversized working-copy content. [Snapshots and undo](../concepts/snapshots-and-undo.md) explains the coverage and limits.

Two verbs cover almost everything here, and telling them apart is the main skill:

- [`ff undo`](../reference/cli/undo.md) moves refs and the working copy together, one step at a time.
- [`ff restore <path>`](../reference/cli/restore.md) writes only worktree files, and leaves refs, HEAD, and the index exactly where they stand.

## "An agent ran `git reset --hard`"

The symptom: an agent, a script, or you in a hurry ran something destructive with raw git, and now the branch points somewhere earlier and the working copy has been rewritten to match.

```console
$ git reset --hard HEAD~2
HEAD is now at 4031999 release: cut v0.1.0
```

The preceding fufu commits recorded the state restored here; this scenario has no uncaptured edits to recover. Raw Git gets a fresh pre-command capture only if an active hook or shell alias invokes fufu first. At the next reconciliation, the reset's ref movement enters the operation log as a foreign operation — [the two regimes](../concepts/two-regimes.md) covers that boundary — and `ff undo` restores the retained state:

```console
$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/parser-stream moved to 4031999d (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at 9ac4e7b60636 (commit on parser-stream: parser: drop whitespace from the stream)
  refs/heads/parser-stream → cc54816d
  2 worktree file(s) restored
back: ff redo
```

Refs and files come back together in the same operation. This is the recovery to reach for whenever the damage is repo-wide and recent, whoever or whatever caused it: `ff undo`, repeated until you are back where you want to be.

## "I want one file back the way it was"

The symptom: one file went wrong — a bad edit, an overzealous refactor — and the rest of the tree is fine, so a repo-wide undo is the wrong tool.

Bare `ff restore <path>` is the everyday "discard my edits to this file": it brings the file back as it stands in the commit under the open change.

```console
$ ff restore src/parser.rs
restored from cc54816d (parser: drop whitespace from the stream)
  restored  src/parser.rs
undo: ff undo
```

`--from <rev>` names a different source — a branch, a SHA, or a [revision expression](../reference/revisions.md#paths-sources-and-past-state-reads) naming one commit. Here, `src/main.rs` as `main` last shipped it:

```console
$ ff restore src/main.rs --from main
restored from 4031999d (release: cut v0.1.0)
  restored  src/main.rs
undo: ff undo
```

Only the worktree is written; branches and HEAD do not move. And a restore takes its own capture first, mandatorily, so a restore that turns out wrong is undone by another restore or by `ff undo`. The same verb also reads from the operation log instead of history: `--at-op <id>` restores a path as one operation held it, and `--at <time>` takes `30m`, `2h`, `3d`, or a date.

## "I want the whole tree from twenty minutes ago"

The symptom: a stretch of work went sideways across many files, and you want the repository as it stood before it started — not one file, and not just the last operation.

[`ff history`](../reference/cli/history.md) is the map of where you can go back to. `@` is where the repository stands; each row below is one more press of `ff undo`. A run of captures collapses into the single row it undoes as, and says how many it collapsed:

```console
$ ff history
@   8221227db90e    0s ago  now   pre: ff status
↓1  9ac4e7b60636    0s ago  undo  commit on parser-stream: parser: drop whitespace from the stream · 4 captures
↓2  9d20ae471524    0s ago  undo  pre: ff commit -m parser: drop whitespace from the stream
↓3  e7bc8c8551d0    0s ago  undo  claim ff/swift-brook as parser-stream
↓4  fc8731f9b6e6    0s ago  undo  commit on ff/swift-brook: parser: skeleton and char stream
↓5  3647923862d8    0s ago  undo  pre: ff commit -m parser: skeleton and char stream
↓6  8a3420b95ce2    0s ago  undo  mint ff/swift-brook at 4031999d and switch from main
↓7  9b92fcff2cc4    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

The twelve-character IDs leading these rows are operation IDs — hexadecimal like a commit's, with the argument selecting the kind: `--at-op` reads an operation, while `-r` and `--from` read revisions. [Revisions and IDs](../reference/revisions.md#operation-expressions) defines the address syntax. Every row is also an address the [`ff op`](../reference/cli/op.md) family takes. [`ff op show`](../reference/cli/op-show.md) confirms a row is the one you mean before anything moves:

```console
$ ff op show 9ac4e7b60636
9ac4e7b60636  op  0s ago
  commit on parser-stream: parser: drop whitespace from the stream
  on        parser-stream
  base      a629e8ce
  refs/heads/parser-stream → cc54816d
  (the worktree is unchanged across it)
```

[`ff op restore <id>`](../reference/cli/op-restore.md) then restores this worktree's recorded local state, subject to worktree guards, instead of pressing undo row by row:

```console
$ ff op restore 9ac4e7b60636
undid: pre: ff status
  now at 9ac4e7b60636 (commit on parser-stream: parser: drop whitespace from the stream)
  2 worktree file(s) restored
back: ff redo
```

When you want only the files from twenty minutes ago and the refs as they are, that is a restore instead: `ff restore --all --at 20m` writes the whole tree from the operation current at that time and moves nothing else.

## "I undid too far"

The symptom: you pressed `ff undo` once more than you meant to, and a commit you wanted is now open again.

```console
$ ff undo
undid: commit on parser-stream: parser: drop whitespace from the stream
  now at 9d20ae471524 (pre: ff commit -m parser: drop whitespace from the stream)
  refs/heads/parser-stream → a629e8ce
back: ff redo
```

Undo moves the log's pointer rather than discarding anything, so [`ff redo`](../reference/cli/redo.md) steps forward again, one run at a time, until the log is back where it started:

```console
$ ff redo
redid: commit on parser-stream: parser: drop whitespace from the stream
  now at 9ac4e7b60636 (commit on parser-stream: parser: drop whitespace from the stream)
  refs/heads/parser-stream → cc54816d
back: ff undo
```

Landing new work after an undo forks the log instead of destroying the path you stepped off. Suppose you undo the same commit again, but this time close it differently rather than redoing:

```console
$ ff undo
undid: commit on parser-stream: parser: drop whitespace from the stream
  now at 9d20ae471524 (pre: ff commit -m parser: drop whitespace from the stream)
  refs/heads/parser-stream → a629e8ce
back: ff redo

$ ff commit -m "parser: drop whitespace and comments"
closed 4a8f5686 on parser-stream: parser: drop whitespace and comments (1 file(s))
undo: ff undo

$ ff redo
ff: nothing to redo: work has landed since the last undo, so the log forked rather than rewound
  try:
    ff op log
    ff undo
```

Redo stops offering a way forward it can no longer take, and says so. Nothing was destroyed: the forked-off branch of the log keeps its ids, `ff op log` still lists them, and `ff op restore` still lands on any of them until [`ff op trim`](../reference/cli/op-trim.md) ages them out.

## "Two writers on one chain, and only one was wrong"

The symptom: two agents share one worktree, so their operations land on one chain in turn. Agent A closed a commit that should not have happened, agent B has since started a branch off trunk and committed there, and `ff undo` would take back B's work first, because undo steps the chain from its newest operation whoever wrote it.

[`ff op log`](../reference/cli/op-log.md) is the chain; find the operation that was wrong:

```console
$ ff op log -n 6
4eb8614a6361   0s ago  op      changelog     commit on changelog: changelog: start one
de4c44e988ad   0s ago  capture changelog     pre: ff commit -m changelog: start one
5df97d7871f3   0s ago  op      changelog     mint changelog at 4031999d and switch from parser-stream
e3daff7e03d8   0s ago  op      parser-stream  commit on parser-stream: README: point at the parser
103a3d5fcd39   0s ago  capture parser-stream  pre: ff commit -m README: point at the parser
37eb8e8cb91a   0s ago  op      parser-stream  commit on parser-stream: parser: drop whitespace and comments
```

[`ff op revert <op>`](../reference/cli/op-revert.md) inverts that one operation and leaves everything after it standing:

```console
$ ff op revert e3daff7e03d8
reverted e3daff7e03d81099843233a71c8f7a5ac023ee47: commit on parser-stream: README: point at the parser
  refs/heads/parser-stream → 4a8f5686
undo: ff undo
```

B's branch and B's commit are untouched, and the revert is itself an operation on the chain, so `ff undo` takes the revert back too.

Revert inverts refs, and it applies only where the refs the wrong operation moved still stand where it left them: a later commit on the same branch moves that ref, so had B committed on parser-stream as well, the revert would hold and change nothing.

That case is a rewrite rather than a recovery. [`ff lift --from <rev>`](../reference/cli/lift.md) takes A's files back out of the closed commit, and drops the commit when it takes everything; [rewriting history](rewriting-history.md#split-a-commit-that-already-closed) has it.

## "I committed to the wrong branch, or with the wrong message"

The symptom: the close itself was fine, but it landed with the wrong name on it — or on the wrong branch entirely.

A wrong message never needs the commit reopened. [`ff describe <rev>`](../reference/cli/describe.md) rewords a commit that has already closed, restacking anything above it in the same operation:

```console
$ ff commit -m wip
closed 30e73a72 on parser-stream: wip (1 file(s))
undo: ff undo

$ ff describe 30e73a72 -m "parser: string literals"
reworded 66f81bba on parser-stream: parser: string literals
undo: ff undo
```

A commit on the wrong branch, if it just happened, is one `ff undo`: the close comes back open, tree and refs together, and you close it again where it belongs — [`ff commit -b <branch>`](../reference/cli/commit.md) lands the close on a fresh branch in the same step.

Once later work has piled on top, this stops being a recovery problem and becomes a rewriting one:

- [`ff absorb --into <rev>`](../reference/cli/absorb.md) folds the open change into the commit it should have joined.
- `ff lift --from <rev>` takes files back out of a closed commit.
- [`ff edit <rev>`](../reference/cli/edit.md) reopens one in place.

[Rewriting history](rewriting-history.md) is the guide for that whole family.

## "Someone force-pushed over my branch"

The symptom: you go to push and fufu refuses — the shared copy of your branch is not where you left it, because somebody rewrote it and force-pushed.

The script first pushes the branch, then a teammate amends and force-pushes its tip from another clone. After a further local commit, [`ff push`](../reference/cli/push.md) refuses because the expected remote tip no longer matches. The local work remains:

```console
$ ff push
ff: origin/parser-stream moved since you last looked, so nothing was pushed — your commits are still here, and ff pull takes in what arrived
  try:
    ff pull parser-stream
    ff push parser-stream
```

[`ff pull`](../reference/cli/pull.md) takes in the rewritten remote tip and replays the later local commit on top. This script's Git amend strips the change-ID header, so this run does not report an identity-based superseded drop:

```console
$ ff pull
fetching from origin
took in 1 commit(s) from origin/parser-stream
replayed 1 of yours on top
1 commit(s) to push — ff push
undo: ff undo
```

Pull sent no remote branch update. Its local branch and file changes are undoable; fetched objects and tracking refs are separate. Once the branch lines up, the push sends it under a fresh lease:

```console
$ ff push
pushed parser-stream to origin/parser-stream
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

The lease checks the remote ref's expected position, not ownership or team policy. fufu can send a rewrite of already-pushed work, including on `main`, when its guards and the server allow it. [The push boundary](../concepts/push-boundary.md) covers the fast-forward exception to the seen-record check, rollback, and `--dry-run`.

## "I need a parked change back with plain git"

A change parked by [`ff switch`](../reference/cli/switch.md) is the branch's open commit, an ordinary commit one above the branch at `refs/fufu/open/<branch>`. With fufu, `ff switch <branch>` resumes it. Without fufu, find it and take it back by hand:

```console
$ git log --all --oneline -3
9f77e40 
a011bfc (main) init
76ddec6 operation log initialized from observed state; earlier operations not undoable

$ git cherry-pick -n 9f77e40
```

The empty subject is the change's pending description, which it had none of. `git cherry-pick -n` lays the change back as uncommitted edits; `git checkout 9f77e40 -- .` takes the whole tree instead.

## What undo cannot reach

Remote effects and uncaptured or expired file states are outside undo's reach. Undo follows this worktree's chain, not another worktree's history.

The first is the push. `ff undo` moves this repository, and a push moves a machine somewhere else — which is why pushing is a verb you type rather than a step riding inside another one, and why every push says so as it goes:

```console
$ ff push
pushed parser-stream to origin/parser-stream
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

A rollback requires another push: undo the commit locally, then push again under the lease and server-side rules. This cannot erase commits already fetched by other clones, CI runs, or webhooks.

The second is the floor. Undo reaches back to the moment fufu started watching, and no further. In a repository fufu adopts rather than creates, the log's first entry says exactly that, and `ff history` marks it:

```console
$ ff init
already a git repository on main
the net is on: ff undo has a floor to land on, and every verb takes one first

$ ff history
@   694b49d3c446    0s ago  now   operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

Everything before fufu's arrival is git's history, not fufu's timeline: reachable with git's own tools, but not a place `ff undo` can land.

The same bound has a sharper edge in day-to-day work. A foreign tree change that moves no ref — a raw `git restore`, an editor discarding a buffer — is invisible until the next capture, so how far back you can reach is set by the last time fufu was looking.

Successful capture and retention determine what can be recovered, even when commands run through fufu. [Snapshots and undo](../concepts/snapshots-and-undo.md) covers those limits.
