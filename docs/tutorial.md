# Tutorial

This walks the whole loop once: get a repository, make commits, switch branches mid-edit, fold a fix into an earlier commit, line up with a teammate, publish, and undo a disaster. Twenty minutes. Every transcript below is real `ff` output.

One thing to unlearn before you start: there is no staging area, no stash, and no dirty state. fufu snapshots the working tree before every action, the tree itself is the change you are working on, and every operation is undoable. You never prepare a commit; you close one.

## Get a repository

[`ff clone`](reference/cli/clone.md) is fufu's own verb, not a wrapper: it speaks the git protocol itself, checks out the worktree, and arms the repository on arrival.

```console
$ ff clone https://github.com/tyler-johnson/fufu
cloned into ./fufu — 265 commits on main
the net is on: ff undo has a floor to land on, and every verb takes one first
```

The second line is the promise the rest of this tutorial leans on. From this moment, every verb takes a snapshot before it acts, so [`ff undo`](reference/cli/undo.md) always has somewhere to land.

The repository you just cloned is fufu's own — real history, real files — so everything below is something you can type, not just read. The work you make here stays in your clone.

That covers fufu's own verbs. An editor edit or a file an agent writes is captured by whatever runs next, so run [`ff hook`](install.md#wire-it-in) if you have not.

If you have a repository git already made, [`ff init`](reference/cli/init.md) inside it means *turn fufu on here* — same arming, nothing else changes. See [Adopting fufu](adopting.md).

## Look around

Bare `ff` is the map: recent work on every branch, parked changes included. A fresh clone is quiet:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
●  —        677b97ae   4m ago
│  cut v0.11.0
~
```

Reading the rows: `@` is the open change — the working tree, as a change in progress. It always exists; `no changes` means the tree matches the commit beneath it. `●` rows are commits, newest first, and `▸ [main]` marks where a branch stands. The `~` says history continues below what is shown.

The letters column next to each commit (here just `—`) is an operation id: which fufu operation last touched that commit. Nothing here has one yet, because fufu didn't make these commits.

## Start work

[`ff start`](reference/cli/start.md) begins a new line of work, always on a fresh branch forked from trunk. There is nothing to name up front — fufu mints a name, and you claim a real one once the work has earned it.

```console
$ ff start
minted ff/vivid-sparrow (forked from main)
open change on ff/vivid-sparrow
undo: ff undo
```

Now edit. Add a file — a design note, say — and notice what you don't do next: no `add`, no staging. Capture is automatic; the working tree is the change.

```console
$ ff status
on ff/vivid-sparrow · nothing to sync
@  mmzzqqkk 29700f0f   0s ago
│  (no description)
│  A notes/parser.md +3  -0  ++++++++++++++++++++
│    1 file          +3  -0
●  —        677b97ae   4m ago  signed
│  cut v0.11.0
```

[`ff status`](reference/cli/status.md) answers where you are and what is uncommitted, as a diffstat. [`ff diff`](reference/cli/diff.md) is the same change read down to the line — and it sees untracked files, which `git diff` does not.

## Name it, then close it

The open change can carry a description before it is ever a commit, so you can name work while you are doing it:

```console
$ ff describe -m "notes: parser skeleton and char stream"
pending description on ff/vivid-sparrow: notes: parser skeleton and char stream
```

Closing the change is the commit. [`ff commit`](reference/cli/commit.md) picks up the pending description:

```console
$ ff commit
closed c709390e on ff/vivid-sparrow: notes: parser skeleton and char stream (1 file(s))
undo: ff undo
```

Or say it at the close. Make a second edit, then:

```console
$ ff commit -m "notes: drop whitespace from the stream"
closed cdb9cf71 on ff/vivid-sparrow: notes: drop whitespace from the stream (1 file(s))
undo: ff undo
```

[`ff log`](reference/cli/log.md) is the changes view for the branch you are on — the open change atop the commit walk; `-n` bounds the rows:

```console
$ ff log -n 5
@  no changes
│  (no description)
●  xmppkont cdb9cf71   0s ago
│  notes: drop whitespace from the stream
●  mmzzqqkk c709390e   0s ago
│  notes: parser skeleton and char stream
●  —        677b97ae   4m ago  signed
│  cut v0.11.0
●  —        ba870bea  34m ago  signed
│  docs: machine-surface names the index and the fifth exit code
●  —        a37965e1  34m ago  signed
│  cli: the error id index, generated from ff explain's registry
```

The two commits fufu made now wear operation ids. [`ff evolog`](reference/cli/evolog.md) drills into a commit's history of rewrites through that column, and [`ff op log`](reference/cli/op-log.md) is the operation log itself.

## Switch without stashing

Start another edit — a stray note in `README.md`, say — and leave mid-thought. Switching parks whatever is open with the branch you are leaving:

```console
$ ff switch main
parked the open change on ff/vivid-sparrow (edb2b4be)
switched to main
undo: ff undo
```

The map shows where the work went:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
│ ●  —        cdb9cf71   0s ago  ▸ [ff/vivid-sparrow]  (+ parked change, 1 file)
│ │  notes: drop whitespace from the stream
│ ●  —        c709390e   0s ago
├─╯  notes: parser skeleton and char stream
●  —        677b97ae   4m ago
│  cut v0.11.0
~
```

Switching back brings the parked change in exactly as you left it — same files, same edits, same pending description. A unique prefix of the branch name is enough for the target.

```console
$ ff switch ff/vivid-sparrow
switched to ff/vivid-sparrow
resumed the parked change (1 file(s))
undo: ff undo
```

The work is real now, so claim the name. The capture chain, the parked state, and any pending description come along — the part a bare `git branch -m` would orphan:

```console
$ ff describe -b parser-stream
claimed ff/vivid-sparrow as parser-stream
undo: ff undo
```

That stray README edit isn't part of this work. [`ff restore`](reference/cli/restore.md) discards one file's edits, back to the commit beneath the change:

```console
$ ff restore README.md
restored from cdb9cf71 (notes: drop whitespace from the stream)
  restored  README.md
undo: ff undo
```

## Fix an earlier commit

Review feedback: the heading you just added belongs in the first commit, not in a new `fixup!` on top. Make the edit, then fold it into the commit it belongs to:

```console
$ ff absorb --into c709390e
absorbed into 8e5e44fc: notes: parser skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The target commit was amended in place and everything above it re-parented in the same operation — no interactive rebase, no autosquash dance, and no file moved on disk. This is the shape of all history rewriting in fufu: you say where the change belongs, and the restacking is automatic. [Rewriting history](guides/rewriting-history.md) has the rest of the family.

## Line up, then send

(This section and the publish below were captured against a copy of the repository with push access — on your clone of fufu, read these two beats along, and replay them the day you point fufu at a repository of your own.)

Meanwhile a teammate landed a commit on `main`. [`ff sync`](reference/cli/sync.md) lines every branch up with both things it answers to: the base beneath it and the remote copy of itself. It fetches once, replays your commits in memory, touches the tree only when the replay is clean, and then reports the other branches it moved, here `main` fast-forwarding to what the teammate pushed:

```console
$ ff sync
fetching from origin
main moved ahead by 1 commit(s)
replayed 2 commit(s) onto main
updated the working tree (1 file(s))
not published yet — ff publish
main
    fast-forwarded to origin/main (1 commit(s))
undo: ff undo
```

Nothing left the machine, and everything sync did is one `ff undo` away. Sending is a separate verb, on purpose — a push cannot be taken back, so it is the one thing you type deliberately:

```console
$ ff publish
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

Every publish carries a lease: it goes through only if the shared copy still stands where you last saw it. In git's terms this is a force-push under a lease, `--force-with-lease`, and the bound is worth stating: fufu never force-pushes `main` or anyone else's branch; it replaces the remote copy of your own branch, and only if nobody has touched it since. If somebody pushed to your branch since, nothing is sent and nothing is lost — `ff sync` takes their work in, and you publish after. [The push boundary](concepts/push-boundary.md) covers leases, rollback, and `--dry-run`.

## Undo anything

fufu snapshots the repository around every operation — including operations it didn't make. So when an overeager agent, or you at 4pm on a Friday, runs something destructive with raw git:

```console
$ git reset --hard HEAD~2
HEAD is now at 3b738f7 docs: a line from a teammate
```

…one `ff undo` brings refs and working tree back together:

```console
$ ff undo
ff: absorbed changes made outside fufu:
  refs/heads/parser-stream moved to 3b738f7f (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at vpyrqznqrozv (published parser-stream to origin/parser-stream)
  refs/heads/parser-stream → 7400e88e
  1 worktree file(s) restored
back: ff redo
```

The reset was never dangerous: fufu snapshotted before it ran, noticed the foreign ref motion, and undid it as if it were any other operation. [`ff redo`](reference/cli/redo.md) goes forward again.

Undo repeats — each press steps one run of work further back. [`ff history`](reference/cli/history.md) is the map of where you can go: `@` is where the repository stands, each row below is one more press of `ff undo`, each row above one more `ff redo`:

```console
$ ff history
↑1  nypvumko    0s ago  redo  absorbed 1 foreign ref change(s)
@   vpyrqznq    0s ago  now   published parser-stream to origin/parser-stream
↓1  umzwnzqv    1s ago  undo  absorb into c709390e on parser-stream
↓2  olpnnlql    1s ago  undo  pre: ff absorb --into c709390e
↓3  nvtnzstr    1s ago  undo  claim ff/vivid-sparrow as parser-stream
↓4  lxnznxsk    1s ago  undo  switch from main to ff/vivid-sparrow
↓5  mqxmpqlx    1s ago  undo  switch from ff/vivid-sparrow to main
↓6  szvwuvzl    1s ago  undo  pre: ff switch main
↓7  pvxxprrn    1s ago  undo  commit on ff/vivid-sparrow: notes: drop whitespace from the stream
↓8  xmppkont    1s ago  undo  pre: ff commit -m notes: drop whitespace from the stream
↓9  nnukmpyx    1s ago  undo  commit on ff/vivid-sparrow: notes: parser skeleton and char stream
↓10 kwuvnqot    1s ago  undo  describe pending change on ff/vivid-sparrow
↓11 mmzzqqkk    1s ago  undo  pre: ff status
↓12 svtwzxnm    1s ago  undo  switch from main to ff/vivid-sparrow
↓13 nlxnpqpl    1s ago  undo  mint branch ff/vivid-sparrow at 677b97ae
↓14 wozpoxmy    1s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

Every row is also an address: [`ff op show <id>`](reference/cli/op-show.md) says what one was, and [`ff op restore <id>`](reference/cli/op-restore.md) lands on it directly instead of pressing undo five times.

## Where you are now

You have the whole loop: `start` begins, `commit` closes, `switch` parks and resumes, `absorb` puts fixes where they belong, `sync` takes in, `publish` sends, and `undo` takes back everything except the push. What you never did: stage, stash, resolve a detached HEAD, or run an interactive rebase.

From here:

- [Changes](concepts/changes.md) and [snapshots and undo](concepts/snapshots-and-undo.md) — the model under what you just did.
- [Recovery](guides/recovery.md) — the undo cookbook for when things are already on fire.
- [fufu vs git](comparisons/vs-git.md) — what changes about your day, and the [command table](comparisons/command-table.md) for reflexes.
- Working alongside people and tools that only speak git: [plain-git teammates](guides/plain-git-teammates.md).
