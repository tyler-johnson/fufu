# Tutorial

This walks the whole loop once: get a repository, make commits, switch branches mid-edit, fold a fix into an earlier commit, line up with a teammate, push, and undo a disaster. Twenty minutes. The examples show real `ff` output; your history, IDs, branch names, and output will differ.

Have [fufu and Git installed](install.md), with your usual Git name and email configured. We'll use a fresh clone of fufu's own repository to experiment in. The pull and push section is a demonstration to follow along with; sending work requires a repository where you have push access.

The working copy is the open change. You can commit it without staging, and switching branches parks uncommitted work. Repository commands and active hooks take snapshots; recovery depends on [what was captured and retained](concepts/snapshots-and-undo.md#coverage-and-limits).

## Get a repository

[`ff clone`](reference/cli/clone.md) gets the repository, checks out the files, and turns fufu on in the new checkout:

```console
$ ff clone https://github.com/tyler-johnson/fufu
cloned into ./fufu — 407 commits on main
the net is on: ff undo has a floor to land on, and every verb takes one first
$ cd fufu
```

The second line says the operation log has been initialized. [`ff undo`](reference/cli/undo.md) can restore local states recorded from that point, while they are retained.

The repository you just cloned is fufu's own — real history, real files — so you have something to look around in and make changes to. The work you make here stays in your clone.

Editor and agent edits are saved by a later successful capture. [`ff hook`](reference/cli/hook.md) installs integrations that add capture attempts as you work; activate them as described in [installation](install.md#wire-it-in) if you have not.

If you have a repository git already made, [`ff init`](reference/cli/init.md) inside it means *turn fufu on here*. See [Adopting fufu](adopting.md).

## Look around

Bare [`ff`](reference/cli/map.md) is the map: recent work on every branch, parked changes included. A fresh clone is quiet:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
●  mrwkzuqt 68614022  19h ago
│  core: close in phases
~
```

Reading the rows: `@` is the open change — the working copy, as a change in progress. It always exists; `no changes` means the tree matches the commit beneath it. `●` rows are commits, newest first, and `▸ [main]` marks where a branch stands. The `~` says history continues below what is shown.

The letters column is the change ID; the hexadecimal value is the commit SHA. A change can keep its identity through rewrites even when its SHA changes. [Revisions and IDs](reference/revisions.md) has the details when you need them.

## Start work

<div class="demo cast" data-cast="../assets/tutorial/start-work.cast">
  <noscript><img src="../assets/tutorial/start-work.gif" alt="ff start creating a branch, an edit to notes/parser.md, and ff status showing the open change"></noscript>
</div>

Let's sketch a design note for a parser. [`ff start`](reference/cli/switch.md) begins a new line of work on a fresh branch forked from trunk — `main` here. There is nothing to name up front; fufu chooses a name, and you can rename it once the work has earned one.

```console
$ ff start
minted ff/bold-hawk (forked from main)
switched to ff/bold-hawk
undo: ff undo
```

Now edit. Add `notes/parser.md` in your editor and jot down a few lines about a character stream feeding a lexer. Save it, then see what [`ff status`](reference/cli/status.md) makes of the change:

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

No `add`, no staging. Status shows where you are and what is uncommitted, as a diffstat. [`ff diff`](reference/cli/diff.md) is the same change read down to the line — and it sees eligible untracked files too.

<a id="commit-your-changes"></a>

## Name it, then close it

<div class="demo cast" data-cast="../assets/tutorial/name-it-then-close-it.cast">
  <noscript><img src="../assets/tutorial/name-it-then-close-it.gif" alt="ff describe naming the change, ff commit closing it, a second commit, and ff log"></noscript>
</div>

The open change can carry a description before it closes, so you can name work while you are doing it. [`ff describe`](reference/cli/describe.md) sets that pending message:

```console
$ ff describe -m "notes: parser skeleton and char stream"
pending description on ff/bold-hawk: notes: parser skeleton and char stream
```

Closing the change records it in branch history. [`ff commit`](reference/cli/commit.md) picks up the pending description:

```console
$ ff commit
closed f12fbdec on ff/bold-hawk: notes: parser skeleton and char stream (1 file(s))
re-minted: signing is on
undo: ff undo
```

This run signs its commits, hence the `re-minted` line. Your signing settings may differ. Either way, your first commit is recorded and the next open change is ready.

Or say it at the close. Add a line at the end of your note saying that the stream drops whitespace before the lexer sees it, then commit that second edit:

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

The two commits fufu made wear the IDs their open changes wore: the letters on the `@` row before each close are the letters on its `●` row after it. [`ff evolog <rev>`](reference/cli/evolog.md) drills into a change's evolution — captures, closing, and later rewrites — and [`ff op log`](reference/cli/op-log.md) is the operation log itself.

## Switch without stashing

<div class="demo cast" data-cast="../assets/tutorial/switch-without-stashing.cast">
  <noscript><img src="../assets/tutorial/switch-without-stashing.gif" alt="ff switch parking a mid-edit change on one branch and resuming it on the other"></noscript>
</div>

Start another edit — a stray note in `README.md`, say — and leave mid-thought. [`ff switch`](reference/cli/switch.md) parks whatever is open with the branch you are leaving:

```console
$ ff switch main
parked the open change on ff/bold-hawk (65f2036f)
switched to main
undo: ff undo
```

The README on `main` has no stray note. The map shows where the work went:

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

Switching back brings the parked change in as you left it — same files, same edits, same pending description. Use the branch name your `ff start` chose:

```console
$ ff switch ff/bold-hawk
switched to ff/bold-hawk
resumed the parked change (1 file(s))
undo: ff undo
```

The work is real now, so give the branch a name. Its saved work and pending description follow the rename:

```console
$ ff describe -b parser-stream
claimed ff/bold-hawk as parser-stream
undo: ff undo
```

That stray README edit isn't part of this work. [`ff restore`](reference/cli/restore.md) captures eligible edits first, then discards the file's edits back to the commit beneath the change:

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

Review feedback: the note needs a heading, and it belongs in the first commit. Add a heading at the top of `notes/parser.md`, away from the line you appended in the second commit. [`ff absorb`](reference/cli/absorb.md) folds the edit into the commit it belongs to; `HEAD~1` is the commit just before the current branch tip:

```console
$ ff absorb --into HEAD~1
moved 1 file(s) from the open change into 33819a31: notes: parser skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The target commit was amended and everything above it replayed in the same operation — no interactive rebase, no autosquash dance, and no file moved on disk. You say where the change belongs, and the restacking is automatic. [Rewriting history](guides/rewriting-history.md) has the rest of the family.

<a id="pull-updates-and-push-your-branch"></a>

## Line up, then send

<div class="demo cast" data-cast="../assets/tutorial/line-up-then-send.cast">
  <noscript><img src="../assets/tutorial/line-up-then-send.gif" alt="ff pull taking in a teammate's commit, then ff push sending the branch"></noscript>
</div>

This is the collaboration part of the loop. Read it along on your clone of fufu, and try it when you work in a repository where you have push access. Cloning the public project does not give you permission to push to it.

Meanwhile a teammate landed a commit on `main`. [`ff pull`](reference/cli/pull.md) lines your current branch up with its base and its remote copy: fetch the new history, bring `main` level with `origin/main`, then replay your two commits on top.

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

The indented report is the other branch it moved, here `main`. If nobody has pushed anything new, there is simply nothing to bring in. Pull sends no branch updates; its local branch and file changes are undoable, while fetched objects and tracking refs are separate.

Your branch is ready to share. [`ff push`](reference/cli/push.md) sends it and sets up tracking:

```console
$ ff push
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

Every push carries a lease: the remote ref must match the expected tip when it is updated. That checks the ref's position, not ownership, and there is no special force-push guard for `main`. Follow your team's shared-history policy. [Pulling and pushing](concepts/push-boundary.md) covers the replacement checks, rollback, and `--dry-run`.

<a id="undo-anything"></a>

## Undo a local operation

<div class="demo cast" data-cast="../assets/tutorial/undo-anything.cast">
  <noscript><img src="../assets/tutorial/undo-anything.gif" alt="git reset --hard removing two commits from the local branch, and ff undo putting refs and the tree back"></noscript>
</div>

Back in your practice clone, try recovering from a mistake. The preceding fufu commands recorded your two commits and their files. With no new edits since that capture, reset the `parser-stream` branch by two commits — the sort of thing an overeager agent, or you at 4pm on a Friday, might do by accident:

```console
$ git reset --hard HEAD~2
HEAD is now at 0ad8617 docs: a line from a teammate
```

The parser note is gone. One `ff undo` brings refs and working copy back together:

```console
$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/parser-stream moved to 0ad8617c (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at 5035c044dda5 (pushed parser-stream to origin/parser-stream)
  refs/heads/parser-stream → 9c1c290f
  1 worktree file(s) restored
back: ff redo
```

fufu noticed the foreign ref motion and restored the retained pre-reset state. Edits made after the last successful capture could still have been lost, and undo cannot change a remote. [Snapshot coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits) covers exclusions and retention. [`ff redo`](reference/cli/redo.md) goes forward again.

Undo repeats — each press steps further back. [`ff history`](reference/cli/history.md) is the map of where you can go: `@` is where the repository stands, each row below is one more `ff undo`, each row above one more `ff redo`. Here is the top of the history from this run:

```console
$ ff history
↑1  30b6292bba9b    0s ago  redo  absorbed 1 foreign ref change(s)
@   5035c044dda5    0s ago  now   pushed parser-stream to origin/parser-stream
↓1  c5ddbdad8c1a    1s ago  undo  move from the open change into f12fbdec on parser-stream
↓2  afe01e6f2b3c    1s ago  undo  pre: ff absorb --into f12fbdec
↓3  83d22784d6bf    1s ago  undo  claim ff/bold-hawk as parser-stream
```

Every row is also an address: [`ff op show <id>`](reference/cli/op-show.md) says what one was, and [`ff op restore <id>`](reference/cli/op-restore.md) lands on it directly instead of pressing undo five times. These hexadecimal addresses are operation IDs; use the ones your history prints.

## Where you are now

You have the whole loop: `start` begins, `commit` closes, `switch` parks and resumes, `absorb` puts fixes where they belong, `pull` takes in, `push` sends, and `undo` restores recorded local work. You did not stage, stash, resolve a detached HEAD, or run an interactive rebase.

From here:

- [Changes](concepts/changes.md) and [snapshots and undo](concepts/snapshots-and-undo.md) — the model under what you just did.
- [Recovery](guides/recovery.md) — the undo cookbook for when things are already on fire.
- [fufu vs git](comparisons/vs-git.md) — what changes about your day, and the [command table](comparisons/command-table.md) for reflexes.
- Working alongside people and tools that only speak git: [plain-Git teammates](guides/plain-git-teammates.md).
