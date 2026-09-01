# Tutorial

This walks the whole loop once: get a repository, make commits, switch branches mid-edit, fold a fix into an earlier commit, line up with a teammate, publish, and undo a disaster. Twenty minutes. Every transcript below is real `ff` output.

One thing to unlearn before you start: there is no staging area, no stash, and no dirty state. fufu snapshots the working tree before every action, the tree itself is the change you are working on, and every operation is undoable. You never prepare a commit; you close one.

## Get a repository

`ff clone` is fufu's own verb, not a wrapper: it speaks the git protocol itself, checks out the worktree, and arms the repository on arrival.

```console
$ ff clone https://example.com/demo.git
cloned into ./demo — 2 commits on main
the net is on: ff undo has a floor to land on, and every verb takes one first
```

The second line is the promise the rest of this tutorial leans on. From this moment, every verb takes a snapshot before it acts, so `ff undo` always has somewhere to land.

That covers fufu's own verbs. An editor edit or a file an agent writes is captured by whatever runs next, so run [`ff hook`](install.md#2-wire-it-in) if you have not.

If you have a repository git already made, `ff init` inside it means *turn fufu on here* — same arming, nothing else changes. See [Adopting fufu](adopting.md).

## Look around

Bare `ff` is the map: recent work on every branch, parked changes included. A fresh clone is quiet:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
●  —        8d58f6b9   2m ago
│  release: cut v0.1.0
~
```

Reading the rows: `@` is the open change — the working tree, as a change in progress. It always exists; `no changes` means the tree matches the commit beneath it. `●` rows are commits, newest first, and `▸ [main]` marks where a branch stands. The `~` says history continues below what is shown.

The letters column next to each commit (here just `—`) is an operation id: which fufu operation last touched that commit. Nothing here has one yet, because fufu didn't make these commits.

## Start work

`ff start` begins a new line of work, always on a fresh branch forked from trunk. There is nothing to name up front — fufu mints a name, and you claim a real one once the work has earned it.

```console
$ ff start
minted ff/hidden-wren (forked from main)
open change on ff/hidden-wren
undo: ff undo
```

Now edit. Add a file, touch another — and notice what you don't do next: no `add`, no staging. Capture is automatic; the working tree is the change.

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

`ff status` answers where you are and what is uncommitted, as a diffstat. `ff diff` is the same change read down to the line — and it sees untracked files, which `git diff` does not.

## Name it, then close it

The open change can carry a description before it is ever a commit, so you can name work while you are doing it:

```console
$ ff describe -m "parser: skeleton and char stream"
pending description on ff/hidden-wren: parser: skeleton and char stream
```

Closing the change is the commit. `ff commit` picks up the pending description:

```console
$ ff commit
closed a223f1f7 on ff/hidden-wren: parser: skeleton and char stream (2 file(s))
undo: ff undo
```

Or say it at the close. Make a second edit, then:

```console
$ ff commit -m "parser: drop whitespace from the stream"
closed 8c8feb4d on ff/hidden-wren: parser: drop whitespace from the stream (1 file(s))
undo: ff undo
```

`ff log` is the changes view for the branch you are on — the open change atop the commit walk:

```console
$ ff log
@  no changes
│  (no description)
●  vyztqtvo 8c8feb4d   1m ago
│  parser: drop whitespace from the stream
●  ozqwnnpu a223f1f7   2m ago
│  parser: skeleton and char stream
●  —        8d58f6b9   6m ago
│  release: cut v0.1.0
●  —        a600c834   6m ago
│  init: hello world
```

The two commits fufu made now wear operation ids. `ff evolog` drills into a commit's history of rewrites through that column, and `ff op log` is the operation log itself.

## Switch without stashing

Start another edit — a stray note in `README.md`, say — and leave mid-thought. Switching parks whatever is open with the branch you are leaving:

```console
$ ff switch main
parked the open change on ff/hidden-wren (0ac71f98)
switched to main
undo: ff undo
```

The map shows where the work went:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
│ ●  —        8c8feb4d   3m ago  ▸ [ff/hidden-wren]  (+ parked change, 1 file)
│ │  parser: drop whitespace from the stream
│ ●  —        a223f1f7   4m ago
├─╯  parser: skeleton and char stream
●  —        8d58f6b9   8m ago
│  release: cut v0.1.0
●  —        a600c834   8m ago
   init: hello world
```

Switching back brings the parked change in exactly as you left it — same files, same edits, same pending description. A unique prefix of the branch name is enough for the target.

```console
$ ff switch ff/hidden-wren
switched to ff/hidden-wren
resumed the parked change (1 file(s))
undo: ff undo
```

The work is real now, so claim the name. The capture chain, the parked state, and any pending description come along — the part a bare `git branch -m` would orphan:

```console
$ ff describe -b parser-stream
claimed ff/hidden-wren as parser-stream
undo: ff undo
```

That stray README edit isn't part of this work. `ff restore` discards one file's edits, back to the commit beneath the change:

```console
$ ff restore README.md
restored from 8c8feb4d (parser: drop whitespace from the stream)
  restored  README.md
undo: ff undo
```

## Fix an earlier commit

Review feedback: the helper you just wrote belongs in the first commit, not in a new `fixup!` on top. Make the edit, then fold it into the commit it belongs to:

```console
$ ff absorb --into a223f1f7
absorbed into 710b3379: parser: skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The target commit was amended in place and everything above it re-parented in the same operation — no interactive rebase, no autosquash dance, and no file moved on disk. This is the shape of all history rewriting in fufu: you say where the change belongs, and the restacking is automatic. [Rewriting history](guides/rewriting-history.md) has the rest of the family.

## Line up, then send

Meanwhile a teammate landed a commit on `main`. `ff sync` lines your branch up with both things it answers to — the base beneath it and the remote copy of itself. It fetches, replays your commits in memory, and touches the tree only when the replay is clean:

```console
$ ff sync
fetching from origin
main moved ahead by 1 commit(s)
replayed 2 commit(s) onto main
updated the working tree (1 file(s))
not published yet — ff publish
undo: ff undo
```

Nothing left the machine, and everything sync did is one `ff undo` away. Sending is a separate verb, on purpose — a push cannot be taken back, so it is the one thing you type deliberately:

```console
$ ff publish
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

Every publish carries a lease: it goes through only if the shared copy still stands where you last saw it. If somebody pushed to your branch since, nothing is sent and nothing is lost — `ff sync` takes their work in, and you publish after. [The push boundary](concepts/push-boundary.md) covers leases, rollback, and `--dry-run`.

## Undo anything

fufu snapshots the repository around every operation — including operations it didn't make. So when an overeager agent, or you at 4pm on a Friday, runs something destructive with raw git:

```console
$ git reset --hard HEAD~2
HEAD is now at 30dc4d4 docs: say what this is
```

…one `ff undo` brings refs and working tree back together:

```console
$ ff undo
ff: absorbed changes made outside fufu:
  refs/heads/parser-stream moved to 30dc4d42 (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at tlkytsulltux (published parser-stream to origin/parser-stream)
  refs/heads/parser-stream → 07fc75ed
  2 worktree file(s) restored
back: ff redo
```

The reset was never dangerous: fufu snapshotted before it ran, noticed the foreign ref motion, and undid it as if it were any other operation. `ff redo` goes forward again.

Undo repeats — each press steps one run of work further back. `ff history` is the map of where you can go: `@` is where the repository stands, each row below is one more press of `ff undo`, each row above one more `ff redo`:

```console
$ ff history
↑1  lmusopvy    1m ago  redo  absorbed 1 foreign ref change(s)
@   tlkytsul    2m ago  now   published parser-stream to origin/parser-stream
↓1  vmpokpuq    4m ago  undo  absorb into a223f1f7 on parser-stream
↓2  pkqxlqoz    4m ago  undo  pre: ff absorb --into a223f1f7
↓3  myyyrvvy    6m ago  undo  claim ff/hidden-wren as parser-stream
↓4  rymuorzu    7m ago  undo  switch from main to ff/hidden-wren
↓5  zstkrutx    8m ago  undo  switch from ff/hidden-wren to main
```

Every row is also an address: `ff op show <id>` says what one was, and `ff op restore <id>` lands on it directly instead of pressing undo five times.

## Where you are now

You have the whole loop: `start` begins, `commit` closes, `switch` parks and resumes, `absorb` puts fixes where they belong, `sync` takes in, `publish` sends, and `undo` takes back everything except the push. What you never did: stage, stash, resolve a detached HEAD, or run an interactive rebase.

From here:

- [Changes](concepts/changes.md) and [snapshots and undo](concepts/snapshots-and-undo.md) — the model under what you just did.
- [Recovery](guides/recovery.md) — the undo cookbook for when things are already on fire.
- [fufu vs git](comparisons/vs-git.md) — what changes about your day, and the [command table](comparisons/command-table.md) for reflexes.
- Working alongside people and tools that only speak git: [plain-git teammates](guides/plain-git-teammates.md).
