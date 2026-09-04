# Rewriting history

Everything you would reach for `rebase -i` for — rewording, squashing, splitting, editing an earlier commit — without the todo list. Every verb here has the same shape: you say where a change belongs, and the restack of everything above the target happens in the same operation.

Each one is a single entry on the operation log, so one [`ff undo`](../reference/cli/undo.md) takes the whole thing back. A replay that would conflict stops with nothing changed and records a [held rewrite](../concepts/held-rewrites.md) — a rewrite parked mid-replay, waiting on you — instead of leaving a stopped rebase on disk.

Every transcript below is real `ff` output from one scratch repository: a `lexer` branch forked from `main`, carrying two commits.

```console
$ ff log
@  no changes
│  (no description)
●  ztkymwnn da11b819   0s ago
│  lexer: drop whitespace
●  nzlsxpsu fc920186   0s ago
│  lexer: skeleton and stream
●  —        7c4c37e9   0s ago
│  release: cut v0.1.0
●  —        cec740bb   0s ago
│  init: hello world
```

## Reword a closed commit

Bare [`ff describe`](../reference/cli/describe.md) names the open change. Naming a revision rewords a commit that has already closed:

```console
$ ff describe fc920186 -m "lexer: skeleton and char stream"
reworded edbf59b6 on lexer: lexer: skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The commit and everything above it now have new ids, because a reword is a rewrite like any other — the tree is untouched, the identity changed. Any branch sitting inside the restacked range comes along with it.

## Fold the open change into a closed commit

Review feedback usually lands on a commit that already closed, and the fix usually lands in your working tree. [`ff absorb`](../reference/cli/absorb.md) folds the open change into the commit it belongs to — the revision you name with `--into`, or the commit under the change when you name none. Here the tree holds two edits: a helper that belongs in the first lexer commit, and a stray note that does not belong anywhere.

```console
$ ff status
on lexer · nothing to sync
@  llkyuukq 1e4f9885   0s ago
│  (no description)
│  M README.md    +2  -0  ++++++++++++++++++++
│  M src/lexer.rs +1  -0  ++++++++++
│    2 files      +3  -0
●  —        2f2985ee   0s ago
│  lexer: drop whitespace
```

An absorb does not attribute hunks: [the change is the unit](../concepts/changes.md), and a path filter chooses which of its files fold in. Whole files are what move, and the rest stays open.

```console
$ ff absorb src/lexer.rs --into edbf59b6
absorbed into 601cd64d: lexer: skeleton and char stream
restacked 1 commit(s) above it
limited to 1 path(s)
the rest of your change is still open
undo: ff undo

$ ff status
on lexer · nothing to sync
@  llkyuukq 92fe2c7a   0s ago
│  (no description)
│  M README.md +2  -0  ++++++++++++++++++++
│    1 file    +2  -0
●  —        b6468fa1   0s ago
│  lexer: drop whitespace
```

The stray note was never part of this work, so discard it:

```console
$ ff restore README.md
restored from b6468fa1 (lexer: drop whitespace)
  restored  README.md
undo: ff undo
```

## Reopen a closed commit

Some fixes cannot be written blind against the tip, because the commit that needs them has been rewritten over since.

[`ff edit`](../reference/cli/edit.md) opens an editing session on a commit: a branch is minted at the commit and you switch to it, so the commit's real content is what sits on disk, with your whole toolchain pointed at it. The branch you came from stays where it stands, its commits waiting ahead, and your open change parks until the session ends.

```console
$ ff edit 601cd64d
editing 601cd64d "lexer: skeleton and char stream" on ff/keen-drake
1 commit(s) wait ahead on lexer
finish with ff done, or ff done --abandon to drop it
undo: ff undo
```

Edit as if the commit were the tip, because for the moment it is. [`ff status`](../reference/cli/status.md) keeps saying where you are and how the session ends:

```console
$ ff status
on ff/keen-drake
editing 601cd64d "lexer: skeleton and char stream" — lands back on lexer
    ff done to finish · ff done --abandon to drop it
@  kxzovkps d610e329   0s ago
│  (no description)
│  M src/lexer.rs +1  -1  ++++++++++----------
│    1 file       +1  -1
●  —        601cd64d   0s ago
│  lexer: skeleton and char stream
```

[`ff done`](../reference/cli/done.md) is one operation: the commit is amended with what the tree now holds, what waited ahead is replayed onto it, and you land back where you left. A replay that would conflict stops with nothing changed rather than leaving you mid-rewrite.

```console
$ ff done
amended 601cd64d "lexer: skeleton and char stream"
replayed 1 commit(s)
back on lexer
undo: ff undo
```

A session you think better of ends with `--abandon`. The session is dropped, and whatever was uncommitted is stashed rather than discarded:

```console
$ ff edit 6e03aa1f
editing 6e03aa1f "lexer: skeleton and char stream" on ff/early-wren
1 commit(s) wait ahead on lexer
finish with ff done, or ff done --abandon to drop it
undo: ff undo

$ ff done --abandon
abandoned the session on 6e03aa1f "lexer: skeleton and char stream"
stashed the session's edits (9aae9296)
back on lexer
undo: ff undo
```

## Split at the close

There is no staging area to assemble a partial commit in. [`ff commit`](../reference/cli/commit.md) takes paths instead — a file or a directory prefix, no globs — and closes a slice, made once at the moment of the close rather than maintained as state. Here the tree holds a parser entry point and a scratchpad of notes:

```console
$ ff status
on lexer · nothing to sync
@  zomlwswu c50d1e82   0s ago
│  (no description)
│  A NOTES.md      +1  -0  ++++++++++++++++++++
│  A src/parser.rs +1  -0  ++++++++++++++++++++
│    2 files       +2  -0
●  —        e3e38eca   0s ago
│  lexer: drop whitespace

$ ff commit src/parser.rs -m "parser: entry point"
closed 89ea1b1c on lexer: parser: entry point (1 file(s))
undo: ff undo
```

What was not named stays open — still the change you are in the middle of:

```console
$ ff status
on lexer · nothing to sync
@  zomlwswu ac73c32a   0s ago
│  (no description)
│  A NOTES.md +1  -0  ++++++++++++++++++++
│    1 file   +1  -0
●  —        89ea1b1c   0s ago
│  parser: entry point

$ ff commit -m "notes: parser scratchpad"
closed 5afd8291 on lexer: notes: parser scratchpad (1 file(s))
undo: ff undo
```

Repeated with narrowing paths, this is how one worktree becomes several layered commits.

## Split a commit that already closed

The close that should have been sliced and was not is the other case. This commit landed two files that belong apart:

```console
$ ff commit -m "parser: eat chars from the stream"
closed c539a953 on lexer: parser: eat chars from the stream (2 file(s))
undo: ff undo

$ ff
@  no changes                  ▸ [lexer]
│  (no description)
●  rylnsknu c539a953   0s ago
│  parser: eat chars from the stream
~  4 commits
●  —        7c4c37e9   0s ago  ▸ [main]
│  release: cut v0.1.0
●  —        cec740bb   0s ago
   init: hello world
```

[`ff lift`](../reference/cli/lift.md) is the other direction of absorb: it takes files back out of a closed commit and into the open change — the revision you name with `--from`, or the commit under the change when you name none. Like absorb, it moves whole files, and a path filter chooses which.

```console
$ ff lift NOTES.md
lifted out of 600b72db: parser: eat chars from the stream
undo: ff undo

$ ff
@  rylnsknu 7c4d4964   0s ago  ▸ [lexer]
│  (no description)
●  —        600b72db   0s ago
│  parser: eat chars from the stream
~  4 commits
●  —        7c4c37e9   0s ago  ▸ [main]
│  release: cut v0.1.0
●  —        cec740bb   0s ago
   init: hello world
```

The map shows the split in progress: the commit stands re-identified with one file fewer, and the lifted file is the open change again, ready to close on its own:

```console
$ ff status
on lexer · nothing to sync
@  rylnsknu 7c4d4964   0s ago
│  (no description)
│  M NOTES.md +1  -0  ++++++++++++++++++++
│    1 file   +1  -0
●  —        600b72db   0s ago
│  parser: eat chars from the stream

$ ff commit -m "notes: eat chars notes"
closed b72ded1a on lexer: notes: eat chars notes (1 file(s))
undo: ff undo
```

Lift with no path takes everything, and fufu writes no empty commit — a lift that empties the commit drops it:

```console
$ ff lift
lifted everything out of b72ded1a "notes: eat chars notes": the commit is gone
undo: ff undo

$ ff undo
undid: lift out of b72ded1a on lexer
  now at vwyvuzqxswwp (commit on lexer: notes: eat chars notes)
  refs/heads/lexer → b72ded1a
back: ff redo
```

That undo is the whole family's safety net in one block: the lift was one operation, so one press put the commit back, tree and refs together.

## Would two branches collide

[`ff collide`](../reference/cli/collide.md) answers a question none of the vertical verbs ask: would these two branches hit each other if both landed? It replays a three-way merge in memory and writes nothing — no index, no worktree, nothing in the object database — so the answer costs a read. A second branch, `renamer`, has appeared beside `lexer`, and both touched `src/main.rs`:

```console
$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  yykyutoy 4eb7c068   0s ago
│  renamer: rename pass
│ ●  —        b72ded1a   2s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  —        7c4c37e9   2s ago  ▸ [main]
│  release: cut v0.1.0
●  —        cec740bb   2s ago
   init: hello world

$ ff collide lexer
  renamer  ✕ lexer  src/main.rs
```

One name means the branch you are on and that one. The verdict names the files that would fight, which makes it a work order for the verbs above: lift the fighting file out of the rename commit and the collision should go with it.

```console
$ ff lift src/main.rs --from 4eb7c068
lifted out of 2cf0b63a: renamer: rename pass
undo: ff undo

$ ff collide lexer
  renamer*  ✕ lexer  src/main.rs
```

Still colliding, and the `*` says why: each side is judged on the tree the operation log holds for it, so uncommitted work counts — the lifted edit is now the open change, and it still fights. That same rule means a branch checked out in another worktree, or nowhere at all, still answers. Discard the lifted edit and ask again:

```console
$ ff restore src/main.rs
restored from 2cf0b63a (renamer: rename pass)
  restored  src/main.rs
undo: ff undo

$ ff collide lexer
  renamer  ✓ lexer

$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  —        2cf0b63a   0s ago
│  renamer: rename pass
│ ●  —        b72ded1a   2s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  —        7c4c37e9   2s ago  ▸ [main]
│  release: cut v0.1.0
●  —        cec740bb   2s ago
   init: hello world
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
  ff/early-wren: branch is gone — pointer removed
  ff/keen-drake: branch is gone — pointer removed

$ ff trim
dropped 33 of 36 operations — previous tip saved at refs/fufu/wt/main/trash/@ops until the next trim
  ff/early-wren: branch is gone — pointer removed
  ff/keen-drake: branch is gone — pointer removed
dropped data frees after gc
```

`-n` is the dry run: the report without a single write. The named branches are the two editing-session branches `ff edit` minted earlier — the sessions ended and the branches are gone, so their pointers into the log go too. The pre-trim tip is saved to a trash ref before anything moves, so the last trim is itself recoverable.

What trim never touches is history. The map after is the map before — same commits, same trees, same messages:

```console
$ ff
@  no changes                  ▸ [renamer]
│  (no description)
●  —        2cf0b63a   3s ago
│  renamer: rename pass
│ ●  —        b72ded1a   5s ago  ▸ [lexer]
│ │  notes: eat chars notes
│ ~  5 commits
├─╯
●  —        7c4c37e9   5s ago  ▸ [main]
│  release: cut v0.1.0
●  —        cec740bb   5s ago
   init: hello world
```

What shrank is where undo can reach. [`ff history`](../reference/cli/history.md) now has a floor where the dropped operations were:

```console
$ ff history
@   zxuwvqoo    0s ago  now   trim: dropped 33 operation(s)
↓1  pxsptmzn    0s ago  undo  switch from lexer to main
↓2  kowpoxox    0s ago  undo  switch from renamer to lexer
    (the floor)
```

## Signed commits are re-signed

A rewrite writes new commits, and a new commit cannot carry the signature the old one had — the signature was over a tree and a set of parents that just moved. Git's answer is `rebase.gpgSign`, which is off by default, so `git rebase` on a signed branch quietly hands back an unsigned one.

fufu's answer is that `commit.gpgsign` governs every commit it writes, replays included. Rewrite a signed commit and the rewrite is signed; restack ten signed commits and ten signed commits come back. There is no separate switch, because a verb that silently unsigned your branch is exactly the failure signing is for.

The cost is one signer run per replayed commit — the same as `git rebase -S`, and worth knowing before restacking twenty commits behind a passphrase-protected key with no agent cached. [Commit signing](../reference/signing.md) has the whole surface, including the environment escape hatch for turning it off for one invocation.

## The append-only boundary

Everything above happened on one machine, which is why all of it was undoable and none of it needed permission. The line where that stops is the push, and [the push boundary](../concepts/push-boundary.md) is where fufu's opinions about malleable history end.

The rewrite verbs themselves do not stop at it. Publish the branch, rewrite a commit the remote now holds, and fufu says so rather than refusing — the rewrite is local, and nothing has left the machine:

```console
$ ff publish
created origin/lexer and set lexer to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease

$ ff describe b72ded1a -m "notes: how eating chars works"
reworded 6147429c on lexer: notes: how eating chars works
1 of the rewritten commits are already on origin/lexer
undo: ff undo
```

Sending that rewrite is [`ff publish`](../reference/cli/publish.md)'s job, and every publish carries a lease: the push goes through only if the shared copy still stands where you last saw it. On your own branch, with nobody else on it, moving the shared copy over your own rewrite is routine:

```console
$ ff publish
published lexer to origin/lexer
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

Now a teammate lands a commit on `origin/lexer`, and the same sequence stops being yours to make. Reword again, and the lease refuses the exit:

```console
$ ff describe 6147429c -m "notes: eating chars, explained"
reworded 37bcef6e on lexer: notes: eating chars, explained
1 of the rewritten commits are already on origin/lexer
undo: ff undo

$ ff publish
ff: origin/lexer moved since you last looked, so nothing was pushed — your commits are still here, and ff sync takes in what arrived
  try:
    ff sync
    ff publish
```

Nothing was sent and nothing was lost. And when [`ff sync`](../reference/cli/sync.md) reconciles, the history the team holds wins: the shared line comes in whole, and a rewrite it already superseded — a different spelling of a commit somebody else has built on — does not survive the replay:

```console
$ ff sync
fetching from origin
took in 2 commit(s) from origin/lexer
replayed 0 of yours on top
dropped 37bcef6e "notes: eating chars, explained" — it changes nothing
updated the working tree (1 file(s))
undo: ff undo
```

That is the boundary in full. fufu has no verb that rewrites history the team shares: the shared copy of a branch moves only through a publish you type, the lease stops a rewrite the moment anyone else has moved the branch, and sync treats the shared line as append-only fact.

Inside your own unpublished work, every commit is malleable and every rewrite is one undo away. The moment other people hold the commits, that stops: the verbs on this page are for the history you have not sent yet. A [held rewrite](../concepts/held-rewrites.md) blocks publish for the same reason — nothing leaves the machine while its history is still about to change under it.

## Where next

- [Stacked changes](stacked-changes.md) — a stack under review, the cascade that carries the branches above every rewrite here, and [`ff restack --onto`](../reference/cli/restack.md) for re-aiming.
- [Recovery](recovery.md) — when a rewrite went wrong and `ff undo` is the verb you want.
- [Held rewrites](../concepts/held-rewrites.md) — what happens when a restack cannot replay cleanly.
