# Tutorial

This walks the whole loop once: get a repository, make commits, switch branches mid-edit, fold a fix into an earlier commit, line up with a teammate, push, and undo a disaster. Twenty minutes. Every transcript below is real `ff` output.

The working copy is the open change. You can commit it without staging, and switching branches parks uncommitted work. Repository commands and active hooks take snapshots; recovery depends on [coverage, capture success, and retention](concepts/snapshots-and-undo.md#coverage-and-limits).

## Get a repository

[`ff clone`](reference/cli/clone.md) is fufu's own verb, not a wrapper: it speaks the git protocol itself, checks out the worktree, and arms the repository on arrival.

```console
$ ff clone https://github.com/tyler-johnson/fufu
cloned into ./fufu — 407 commits on main
the net is on: ff undo has a floor to land on, and every verb takes one first
```

The second line says the operation log has been initialized. [`ff undo`](reference/cli/undo.md) can restore the local states recorded from that point, while they are retained.

The repository you just cloned is fufu's own — real history, real files — so everything below is something you can type, not just read. The work you make here stays in your clone.

Editor and agent edits need a later successful capture to be saved. [`ff hook`](reference/cli/hook.md) installs integrations that attempt capture on supported events; activate them as described in [installation](install.md#wire-it-in).

If you have a repository git already made, [`ff init`](reference/cli/init.md) inside it means *turn fufu on here* — same arming, nothing else changes. See [Adopting fufu](adopting.md).

## Look around

Bare `ff` is the map: recent work on every branch, parked changes included. A fresh clone is quiet:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
●  mrwkzuqt 68614022  19h ago
│  core: close in phases
~
```

Reading the rows: `@` is the open change — the working copy, as a change in progress. It always exists; `no changes` means the tree matches the commit beneath it. `●` rows are commits, newest first, and `▸ [main]` marks where a branch stands. The `~` says history continues below what is shown.

The letters column next to each commit is its change id: the identity a commit keeps through rewrites. Every commit has one. fufu mints its own and writes it into the commit; the rest, made by git or cloned from elsewhere, derive theirs from the sha, so every clone agrees.

## Start work

<div class="demo cast" data-cast="../assets/tutorial/start-work.cast">
  <noscript><img src="../assets/tutorial/start-work.gif" alt="ff start minting a branch, an edit to notes/parser.md, and ff status showing the open change"></noscript>
</div>

[`ff start`](reference/cli/switch.md) begins a new line of work on a fresh branch forked from trunk — it is `ff switch` with no branch to find, so one is minted. There is nothing to name up front — fufu mints a name, and you claim a real one once the work has earned it.

```console
$ ff start
minted ff/bold-hawk (forked from main)
switched to ff/bold-hawk
undo: ff undo
```

Now edit. Add a file — a design note, say — and notice what you don't do next: no `add`, no staging. Capture is automatic; the working copy is the change.

```console
$ ff status
on ff/bold-hawk · nothing to pull
@  urrumwkl            0s ago
│  (no description)
│  A notes/parser.md +3  -0  ++++++++++++++++++++
│    1 file          +3  -0
●  mrwkzuqt 68614022  19h ago  signed
│  core: close in phases
```

[`ff status`](reference/cli/status.md) answers where you are and what is uncommitted, as a diffstat. [`ff diff`](reference/cli/diff.md) is the same change read down to the line — and it sees untracked files, which `git diff` does not.

## Name it, then close it

<div class="demo cast" data-cast="../assets/tutorial/name-it-then-close-it.cast">
  <noscript><img src="../assets/tutorial/name-it-then-close-it.gif" alt="ff describe naming the change, ff commit closing it, a second commit, and ff log"></noscript>
</div>

The open change can carry a description before it closes, so you can name work while you are doing it:

```console
$ ff describe -m "notes: parser skeleton and char stream"
pending description on ff/bold-hawk: notes: parser skeleton and char stream
```

Closing the change is the commit. [`ff commit`](reference/cli/commit.md) picks up the pending description:

```console
$ ff commit
closed f12fbdec on ff/bold-hawk: notes: parser skeleton and char stream (1 file(s))
re-minted: signing is on
undo: ff undo
```

The `re-minted` line is the signing user's: the open change is already a commit, the one the `@` row's sha names, and a close usually moves the branch onto it. Ada signs her commits, so the close signs a fresh one instead and says so.

Or say it at the close. Make a second edit, then:

```console
$ ff commit -m "notes: drop whitespace from the stream"
closed bb595ed7 on ff/bold-hawk: notes: drop whitespace from the stream (1 file(s))
re-minted: signing is on
undo: ff undo
```

[`ff log`](reference/cli/log.md) is the changes view for the branch you are on — the open change atop the commit walk; `-n` bounds the rows:

```console
$ ff log -n 5
@  no changes
│  (no description)
●  tlwwsulp bb595ed7   0s ago  signed
│  notes: drop whitespace from the stream
●  urrumwkl f12fbdec   1s ago  signed
│  notes: parser skeleton and char stream
●  mrwkzuqt 68614022  19h ago  signed
│  core: close in phases
●  wrnspylp c1518a8b  19h ago  signed
│  core: rewind's steps are functions
●  zmnkwzuk 670dd3e4  19h ago  signed
│  core: done_with and finish_resolution in phases
```

The two commits fufu made wear the ids their open changes wore: the letters on the `@` row before each close are the letters on its `●` row after it. [`ff evolog <rev>`](reference/cli/evolog.md) drills into a commit's history through that column — the close, every later rewrite, and the captures behind it — and [`ff op log`](reference/cli/op-log.md) is the operation log itself.

## Switch without stashing

<div class="demo cast" data-cast="../assets/tutorial/switch-without-stashing.cast">
  <noscript><img src="../assets/tutorial/switch-without-stashing.gif" alt="ff switch parking a mid-edit change on one branch and resuming it on the other"></noscript>
</div>

Start another edit — a stray note in `README.md`, say — and leave mid-thought. Switching parks whatever is open with the branch you are leaving:

```console
$ ff switch main
parked the open change on ff/bold-hawk (65f2036f)
switched to main
undo: ff undo
```

The map shows where the work went:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
│ ●  tlwwsulp bb595ed7   0s ago  ▸ [ff/bold-hawk]  (+ parked change, 1 file)
│ │  notes: drop whitespace from the stream
│ ●  urrumwkl f12fbdec   0s ago
├─╯  notes: parser skeleton and char stream
●  mrwkzuqt 68614022  19h ago
│  core: close in phases
~
```

Switching back brings the parked change in exactly as you left it — same files, same edits, same pending description. A unique prefix of the branch name is enough for the target.

```console
$ ff switch ff/bold-hawk
switched to ff/bold-hawk
resumed the parked change (1 file(s))
undo: ff undo
```

The work is real now, so claim the name. The capture chain, the parked state, and any pending description come along — the part a bare `git branch -m` would orphan:

```console
$ ff describe -b parser-stream
claimed ff/bold-hawk as parser-stream
undo: ff undo
```

That stray README edit isn't part of this work. [`ff restore`](reference/cli/restore.md) discards one file's edits, back to the commit beneath the change:

```console
$ ff restore README.md
restored from bb595ed7 (notes: drop whitespace from the stream)
  restored  README.md
undo: ff undo
```

## Fix an earlier commit

<div class="demo cast" data-cast="../assets/tutorial/fix-an-earlier-commit.cast">
  <noscript><img src="../assets/tutorial/fix-an-earlier-commit.gif" alt="ff log finding the commit, an edit, and ff absorb folding it into that commit"></noscript>
</div>

Review feedback: the heading you just added belongs in the first commit, not in a new `fixup!` on top. Make the edit, then fold it into the commit it belongs to:

```console
$ ff absorb --into f12fbdec
moved 1 file(s) from the open change into 33819a31: notes: parser skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The target commit was amended in place and everything above it re-parented in the same operation — no interactive rebase, no autosquash dance, and no file moved on disk. This is the shape of all history rewriting in fufu: you say where the change belongs, and the restacking is automatic. [Rewriting history](guides/rewriting-history.md) has the rest of the family.

## Line up, then send

<div class="demo cast" data-cast="../assets/tutorial/line-up-then-send.cast">
  <noscript><img src="../assets/tutorial/line-up-then-send.gif" alt="ff pull taking in a teammate's commit, then ff push sending the branch"></noscript>
</div>

(This section and the push below were captured against a copy of the repository with push access — on your clone of fufu, read these two beats along, and replay them the day you point fufu at a repository of your own.)

Meanwhile a teammate landed a commit on `main`. [`ff pull`](reference/cli/pull.md) lines the branch you stand on up with both things it answers to: the base beneath it and the remote copy of itself. It fetches once, brings the base level with its own remote copy, replays your commits in memory, touches the tree only when the replay is clean, and then reports the other branches it moved, here `main` fast-forwarding to what the teammate pushed:

```console
$ ff pull
fetching from origin
main moved ahead by 1 commit(s)
replayed 2 commit(s) onto main
updated the working copy (1 file(s))
not published yet — ff push
main
    fast-forwarded to origin/main (1 commit(s))
undo: ff undo
```

Pull sent no remote branch update, and its local branch and file changes are one `ff undo` away. Fetched objects and tracking refs are separate. Sending is a separate verb because undo cannot reach the remote:

```console
$ ff push
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

Every push carries a lease: the remote ref must match the expected tip when Git updates it. Replacing commits also checks fufu's seen record; a fast-forward can proceed without that agreement. In Git's terms the wire update uses `--force-with-lease`.

The lease checks the remote ref's expected position. It does not check branch ownership, and fufu has no special force-push guard for `main`. Follow your team's shared-history policy and use server-side branch protection where rewrites must be refused.

If somebody pushed to your branch since, nothing is sent and nothing is lost — `ff pull` takes their work in, and you push after. [The push boundary](concepts/push-boundary.md) covers leases, rollback, and `--dry-run`.

## Undo anything

<div class="demo cast" data-cast="../assets/tutorial/undo-anything.cast">
  <noscript><img src="../assets/tutorial/undo-anything.gif" alt="git reset --hard destroying two commits, and ff undo putting refs and the tree back"></noscript>
</div>

The work in this example was captured by the preceding fufu commands. Raw Git only gets a fresh pre-command snapshot when an active agent hook or the shell alias invokes fufu first. Here, resetting the already-recorded branch demonstrates recovery from that saved state:

```console
$ git reset --hard HEAD~2
HEAD is now at 0ad8617 docs: a line from a teammate
```

…one `ff undo` brings refs and working copy back together:

```console
$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/parser-stream moved to 0ad8617c (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at 5035c044dda5 (pushed parser-stream to origin/parser-stream)
  refs/heads/parser-stream → 9c1c290f
  1 worktree file(s) restored
back: ff redo
```

fufu noticed the foreign ref motion and restored the retained pre-reset state. Edits made after the last successful capture could still have been lost. Snapshots exclude ignored untracked files and cap oversized working-copy content; retention limits their lifetime. Undo follows this worktree's chain and cannot reverse remote effects. [Snapshot coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits) has the details. [`ff redo`](reference/cli/redo.md) goes forward again.

Undo repeats — each press steps one run of work further back. [`ff history`](reference/cli/history.md) is the map of where you can go: `@` is where the repository stands, each row below is one more press of `ff undo`, each row above one more `ff redo`:

```console
$ ff history
↑1  30b6292bba9b    0s ago  redo  absorbed 1 foreign ref change(s)
@   5035c044dda5    0s ago  now   pushed parser-stream to origin/parser-stream
↓1  c5ddbdad8c1a    1s ago  undo  move from the open change into f12fbdec on parser-stream
↓2  afe01e6f2b3c    1s ago  undo  pre: ff absorb --into f12fbdec
↓3  83d22784d6bf    1s ago  undo  claim ff/bold-hawk as parser-stream
↓4  410f2b8b34b6    1s ago  undo  switch from main to ff/bold-hawk
↓5  5162b667b0bf    1s ago  undo  switch from ff/bold-hawk to main
↓6  3329a299c602    1s ago  undo  pre: ff switch main
↓7  acca545e81a8    1s ago  undo  commit on ff/bold-hawk: notes: drop whitespace from the stream
↓8  0b18bb7d2ca4    1s ago  undo  pre: ff commit -m notes: drop whitespace from the stream
↓9  2f31ad9c6a5d    1s ago  undo  commit on ff/bold-hawk: notes: parser skeleton and char stream
↓10 6b0cfd0631a8    2s ago  undo  describe pending change on ff/bold-hawk
↓11 7438353ddc76    2s ago  undo  pre: ff status
↓12 1f2f3c75b39b    2s ago  undo  mint ff/bold-hawk at 68614022 and switch from main
↓13 87c42cadf3eb    2s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

Every row is also an address: [`ff op show <id>`](reference/cli/op-show.md) says what one was, and [`ff op restore <id>`](reference/cli/op-restore.md) lands on it directly instead of pressing undo five times.

## Where you are now

You have the whole loop: `start` begins, `commit` closes, `switch` parks and resumes, `absorb` puts fixes where they belong, `pull` takes in, `push` sends, and `undo` restores recorded local work. You did not stage, stash, resolve a detached HEAD, or run an interactive rebase.

From here:

- [Changes](concepts/changes.md) and [snapshots and undo](concepts/snapshots-and-undo.md) — the model under what you just did.
- [Recovery](guides/recovery.md) — the undo cookbook for when things are already on fire.
- [fufu vs git](comparisons/vs-git.md) — what changes about your day, and the [command table](comparisons/command-table.md) for reflexes.
- Working alongside people and tools that only speak git: [plain-git teammates](guides/plain-git-teammates.md).
