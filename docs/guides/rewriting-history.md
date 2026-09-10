# Rewriting history

Everything you would reach for `rebase -i` for — rewording, squashing, splitting, editing an earlier commit — without the todo list. Every verb here has the same shape: you say where a change belongs, and the restack of everything above the target happens in the same operation.

Each one is a single entry on the operation log, so one [`ff undo`](../reference/cli/undo.md) takes the whole thing back. A replay that would conflict stops with nothing changed and records a [held rewrite](../concepts/held-rewrites.md) — a rewrite parked mid-replay, waiting on you — instead of leaving a stopped rebase on disk.

Every transcript below is real `ff` output from one scratch repository: a `lexer` branch forked from `main`, carrying two commits.

```console
$ ff log
@  no changes
│  (no description)
●  zrwvnqnr 6c2d2362   0s ago
│  lexer: drop whitespace
●  lwqussmu 342875b2   0s ago
│  lexer: skeleton and stream
●  kmxqpvvk 9288324a   1s ago
│  release: cut v0.1.0
●  zvunwxsk 5abc3ebf   1s ago
│  init: hello world
```

## Reword a closed commit

Bare [`ff describe`](../reference/cli/describe.md) names the open change. Naming a revision rewords a commit that has already closed:

```console
$ ff describe 342875b2 -m "lexer: skeleton and char stream"
reworded 6d971d9f on lexer: lexer: skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The commit and everything above it now have new ids, because a reword is a rewrite like any other — the tree is untouched, the identity changed. Any branch sitting inside the restacked range comes along with it.

## Fold the open change into a closed commit

Review feedback usually lands on a commit that already closed, and the fix usually lands in your working copy. [`ff absorb`](../reference/cli/absorb.md) folds the open change into the commit it belongs to — the revision you name with `--into`, or the commit under the change when you name none. Here the tree holds two edits: a helper that belongs in the first lexer commit, and a stray note that does not belong anywhere.

```console
$ ff status
on lexer · nothing to pull
@  ntytokxr 9f2db953   0s ago
│  (no description)
│  M README.md    +2  -0  ++++++++++++++++++++
│  M src/lexer.rs +1  -0  ++++++++++
│    2 files      +3  -0
●  zrwvnqnr a2d068c3   0s ago
│  lexer: drop whitespace
```

An absorb does not attribute hunks: [the change is the unit](../concepts/changes.md), and a path filter chooses which of its files fold in. Whole files are what move, and the rest stays open.

```console
$ ff absorb src/lexer.rs --into 6d971d9f
absorbed into ef7efd19: lexer: skeleton and char stream
restacked 1 commit(s) above it
limited to 1 path(s)
the rest of your change is still open
undo: ff undo

$ ff status
on lexer · nothing to pull
@  ntytokxr 19693b74   0s ago
│  (no description)
│  M README.md +2  -0  ++++++++++++++++++++
│    1 file    +2  -0
●  zrwvnqnr 8968ba9f   0s ago
│  lexer: drop whitespace
```

The stray note was never part of this work, so discard it:

```console
$ ff restore README.md
restored from 8968ba9f (lexer: drop whitespace)
  restored  README.md
undo: ff undo
```

## Reopen a closed commit

Some fixes cannot be written blind against the tip, because the commit that needs them has been rewritten over since.

[`ff edit`](../reference/cli/edit.md) opens an editing session on a commit: a branch is minted at the commit and you switch to it, so the commit's real content is what sits on disk, with your whole toolchain pointed at it. The branch you came from stays where it stands, its commits waiting ahead, and your open change parks until the session ends.

```console
$ ff edit ef7efd19
editing ef7efd19 "lexer: skeleton and char stream" on ff/gentle-owl
1 commit(s) wait ahead on lexer
finish with ff done, or ff done --abandon to drop it
undo: ff undo
```

Edit as if the commit were the tip, because for the moment it is. [`ff status`](../reference/cli/status.md) keeps saying where you are and how the session ends:

```console
$ ff status
on ff/gentle-owl
editing ef7efd19 "lexer: skeleton and char stream" — lands back on lexer
    ff done to finish · ff done --abandon to drop it
@  lwqussmu a51dea2f   0s ago
│  (no description)
│  M src/lexer.rs +1  -1  ++++++++++----------
│    1 file       +1  -1
●  lwqussmu ef7efd19   0s ago
│  lexer: skeleton and char stream
```

[`ff done`](../reference/cli/done.md) is one operation: the commit is amended with what the tree now holds, what waited ahead is replayed onto it, and you land back where you left. A replay that would conflict stops with nothing changed rather than leaving you mid-rewrite.

```console
$ ff done
amended ef7efd19 "lexer: skeleton and char stream"
replayed 1 commit(s)
back on lexer
undo: ff undo
```

A session you think better of ends with `--abandon`. The session is dropped, and whatever was uncommitted is stashed rather than discarded:

```console
$ ff edit c9cad0c8
editing c9cad0c8 "lexer: skeleton and char stream" on ff/early-spruce
1 commit(s) wait ahead on lexer
finish with ff done, or ff done --abandon to drop it
undo: ff undo

$ ff done --abandon
abandoned the session on c9cad0c8 "lexer: skeleton and char stream"
stashed the session's edits (d81d7eb9)
back on lexer
undo: ff undo
```

## Split at the close

There is no staging area to assemble a partial commit in. [`ff commit`](../reference/cli/commit.md) takes paths instead — a file or a directory prefix, no globs — and closes a slice, made once at the moment of the close rather than maintained as state. Here the tree holds a parser entry point and a scratchpad of notes:

```console
$ ff status
on lexer · nothing to pull
@  ntytokxr da77bfc6   0s ago
│  (no description)
│  A NOTES.md      +1  -0  ++++++++++++++++++++
│  A src/parser.rs +1  -0  ++++++++++++++++++++
│    2 files       +2  -0
●  zrwvnqnr 4b55de54   0s ago
│  lexer: drop whitespace

$ ff commit src/parser.rs -m "parser: entry point"
closed 8667585e on lexer: parser: entry point (1 file(s))
undo: ff undo
```

What was not named stays open — still the change you are in the middle of:

```console
$ ff status
on lexer · nothing to pull
@  mmknntln 137647a9   0s ago
│  (no description)
│  A NOTES.md +1  -0  ++++++++++++++++++++
│    1 file   +1  -0
●  ntytokxr 8667585e   0s ago
│  parser: entry point

$ ff commit -m "notes: parser scratchpad"
closed 36a6b15c on lexer: notes: parser scratchpad (1 file(s))
undo: ff undo
```

Repeated with narrowing paths, this is how one worktree becomes several layered commits.

## Split a commit that already closed

The close that should have been sliced and was not is the other case. This commit landed two files that belong apart:

```console
$ ff commit -m "parser: eat chars from the stream"
closed 1bc473ea on lexer: parser: eat chars from the stream (2 file(s))
undo: ff undo

$ ff
@  no changes                  ▸ [lexer]
│  (no description)
●  ossslovp 1bc473ea   0s ago
│  parser: eat chars from the stream
~  4 commits
●  kmxqpvvk 9288324a   1s ago  ▸ [main]
│  release: cut v0.1.0
~
```

[`ff lift`](../reference/cli/lift.md) is the other direction of absorb: it takes files back out of a closed commit and into the open change — the revision you name with `--from`, or the commit under the change when you name none. Like absorb, it moves whole files, and a path filter chooses which.

```console
$ ff lift NOTES.md
lifted out of c7cc9350: parser: eat chars from the stream
undo: ff undo

$ ff
@  —                   0s ago  ▸ [lexer]
│  (no description)
●  ossslovp c7cc9350   0s ago
│  parser: eat chars from the stream
~  4 commits
●  kmxqpvvk 9288324a   1s ago  ▸ [main]
│  release: cut v0.1.0
~
```

The map shows the split in progress: the commit stands re-identified with one file fewer, and the lifted file is the open change again, ready to close on its own:

```console
$ ff status
on lexer · nothing to pull
@  —                   0s ago
│  (no description)
│  M NOTES.md +1  -0  ++++++++++++++++++++
│    1 file   +1  -0
●  ossslovp c7cc9350   0s ago
│  parser: eat chars from the stream

$ ff commit -m "notes: eat chars notes"
closed 493f101e on lexer: notes: eat chars notes (1 file(s))
undo: ff undo
```

Lift with no path takes everything, and fufu writes no empty commit — a lift that empties the commit drops it:

```console
$ ff lift
lifted everything out of 493f101e "notes: eat chars notes": the commit is gone
undo: ff undo

$ ff undo
undid: lift out of 493f101e on lexer
  now at 313eee596f39 (commit on lexer: notes: eat chars notes)
  refs/heads/lexer → 493f101e
back: ff redo
```

That undo is the whole family's safety net in one block: the lift was one operation, so one press put the commit back, tree and refs together.

## Would two branches collide

[`ff collide`](../reference/cli/collide.md) answers a question none of the vertical verbs ask: would these two branches hit each other if both landed? It replays a three-way merge in memory and writes nothing — no index, no worktree, nothing in the object database — so the answer costs a read. A second branch, `renamer`, has appeared beside `lexer`, and both touched `src/main.rs`:

```console
$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  lnwtsxtl a1bec1a5   0s ago
│  renamer: rename pass
│ ●  ytsrlmwp 493f101e   1s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  kmxqpvvk 9288324a   2s ago  ▸ [main]
│  release: cut v0.1.0
~

$ ff collide lexer
  renamer  ✕ lexer  src/main.rs
```

One name means the branch you are on and that one. The verdict names the files that would fight, which makes it a work order for the verbs above: lift the fighting file out of the rename commit and the collision should go with it.

```console
$ ff lift src/main.rs --from a1bec1a5
lifted out of 6bcd2ced: renamer: rename pass
undo: ff undo

$ ff collide lexer
  renamer*  ✕ lexer  src/main.rs

  * has uncommitted work
```

Still colliding, and the `*` says why: each side is judged on the tree the operation log holds for it, so uncommitted work counts — the lifted edit is now the open change, and it still fights. That same rule means a branch checked out in another worktree, or nowhere at all, still answers. Discard the lifted edit and ask again:

```console
$ ff restore src/main.rs
restored from 6bcd2ced (renamer: rename pass)
  restored  src/main.rs
undo: ff undo

$ ff collide lexer
  renamer  ✓ lexer

$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  lnwtsxtl 6bcd2ced   0s ago
│  renamer: rename pass
│ ●  ytsrlmwp 493f101e   1s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  kmxqpvvk 9288324a   2s ago  ▸ [main]
│  release: cut v0.1.0
~
```

A collision is a finding rather than a failure: the exit is 0 whichever way the answer goes, and a script reads the verdict from `--json`.

## Trim the operation log

Every rewrite above landed as an operation, and [operations are what make it all undoable](../concepts/snapshots-and-undo.md). [`ff trim`](../reference/cli/trim.md) is the retention pass that keeps that log from growing forever: operations older than the `fufu.keep` window (90 days by default) are dropped, and a trim rides an ordinary ff command at most once per day, so you rarely run it by hand. With everything inside the window there is nothing to do:

```console
$ ff trim -n
nothing to drop (33 operations kept)
```

To make retention visible inside one transcript, this scene shrinks the window to seconds — a real setting looks like [`ff config keep 30d`](../reference/cli/config.md):

```console
$ ff config keep 2s
keep = 2s (this repo)

$ ff trim -n
would drop 33 of 36 operations
  ff/early-spruce: branch is gone — pointer removed
  ff/gentle-owl: branch is gone — pointer removed

$ ff trim
dropped 33 of 36 operations — previous tip saved at refs/fufu/wt/main/trash/@ops until the next trim
  ff/early-spruce: branch is gone — pointer removed
  ff/gentle-owl: branch is gone — pointer removed
dropped data frees after gc
```

`-n` is the dry run: the report without a single write. The named branches are the two editing-session branches `ff edit` minted earlier — the sessions ended and the branches are gone, so their pointers into the log go too. The pre-trim tip is saved to a trash ref before anything moves, so the last trim is itself recoverable.

What trim never touches is history. The map after is the map before — same commits, same trees, same messages:

```console
$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  lnwtsxtl 6bcd2ced   3s ago
│  renamer: rename pass
│ ●  ytsrlmwp 493f101e   4s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  kmxqpvvk 9288324a   5s ago  ▸ [main]
│  release: cut v0.1.0
~
```

What shrank is where undo can reach. [`ff history`](../reference/cli/history.md) now has a floor where the dropped operations were:

```console
$ ff history
@   26f0d1b90cc3    0s ago  now   trim: dropped 33 operation(s)
↓1  122c2d118e99    0s ago  undo  switch from lexer to main [1b234d04-d951-438c-9b46-3de76978f90d]
↓2  dad2dd7e5764    0s ago  undo  switch from renamer to lexer [1b234d04-d951-438c-9b46-3de76978f90d]
    (the floor)
```

## Signed commits are re-signed

A rewrite writes new commits, and a new commit cannot carry the signature the old one had — the signature was over a tree and a set of parents that just moved. Git's answer is `rebase.gpgSign`, which is off by default, so `git rebase` on a signed branch quietly hands back an unsigned one.

fufu's answer is that `commit.gpgsign` governs every commit it writes, replays included. Rewrite a signed commit and the rewrite is signed; restack ten signed commits and ten signed commits come back. There is no separate switch, because a verb that silently unsigned your branch is exactly the failure signing is for.

The cost is one signer run per replayed commit — the same as `git rebase -S`, and worth knowing before restacking twenty commits behind a passphrase-protected key with no agent cached. [Commit signing](../reference/signing.md) has the whole surface, including the environment escape hatch for turning it off for one invocation.

## The append-only boundary

Everything above happened on one machine, which is why all of it was undoable and none of it needed permission. The line where that stops is the push, and [the push boundary](../concepts/push-boundary.md) is where fufu's opinions about malleable history end.

The rewrite verbs themselves do not stop at it. Push the branch, rewrite a commit the remote now holds, and fufu says so rather than refusing — the rewrite is local, and nothing has left the machine:

```console
$ ff push
created origin/lexer and set lexer to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

$ ff describe 493f101e -m "notes: how eating chars works"
reworded cb89b2dc on lexer: notes: how eating chars works
1 of the rewritten commits are already on origin/lexer
undo: ff undo
```

Sending that rewrite is [`ff push`](../reference/cli/push.md)'s job, and every push carries a lease: the push goes through only if the shared copy still stands where you last saw it. On your own branch, with nobody else on it, moving the shared copy over your own rewrite is routine:

```console
$ ff push
pushed lexer to origin/lexer
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

Now a teammate lands a commit on `origin/lexer`, and the same sequence stops being yours to make. Reword again, and the lease refuses the exit:

```console
$ ff describe cb89b2dc -m "notes: eating chars, explained"
reworded b31fd770 on lexer: notes: eating chars, explained
1 of the rewritten commits are already on origin/lexer
undo: ff undo

$ ff push
ff: origin/lexer moved since you last looked, so nothing was pushed — your commits are still here, and ff pull takes in what arrived
  try:
    ff pull lexer
    ff push lexer
```

Nothing was sent and nothing was lost. And when [`ff pull`](../reference/cli/pull.md) reconciles, the history the team holds wins: the shared line comes in whole, and a rewrite it already superseded — a different spelling of a commit somebody else has built on — does not survive the replay. The change id decides it: your commit and the teammate's rewrite of it carry the same id, so yours is dropped as superseded without a merge, whatever either spelling's content:

```console
$ ff pull
fetching from origin
took in 2 commit(s) from origin/lexer
replayed 0 of yours on top
dropped b31fd770 "notes: eating chars, explained" — superseded by cb89b2dc in the base
updated the working copy (1 file(s))
undo: ff undo
```

That is the boundary in full. fufu has no verb that rewrites history the team shares: the shared copy of a branch moves only through a push you type, the lease stops a rewrite the moment anyone else has moved the branch, and pull treats the shared line as append-only fact.

Inside your own unpublished work, every commit is malleable and every rewrite is one undo away. The moment other people hold the commits, that stops: the verbs on this page are for the history you have not sent yet. A [held rewrite](../concepts/held-rewrites.md) blocks the push for the same reason — nothing leaves the machine while its history is still about to change under it.

## Where next

- [Stacked changes](stacked-changes.md) — a stack under review, the cascade that carries the branches above every rewrite here, and [`ff restack --onto`](../reference/cli/restack.md) for re-aiming.
- [Recovery](recovery.md) — when a rewrite went wrong and `ff undo` is the verb you want.
- [Held rewrites](../concepts/held-rewrites.md) — what happens when a restack cannot replay cleanly.
