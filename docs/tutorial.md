# Tutorial

Make two commits, switch branches mid-edit, fix an earlier commit, pull an update, push your branch, and undo a local reset. Allow about twenty minutes. Every edit is supplied, and the transcripts come from the [shared command script](https://github.com/tyler-johnson/fufu/blob/main/scripts/docs/tutorial-steps.sh).

You need [fufu installed](install.md), Git, and Bash with standard Unix tools (Linux, macOS, or Git Bash on Windows). The exercise creates a tiny project, a bare repository to serve as its remote, and a second clone to simulate a teammate. Everything stays in a temporary directory: no hosting account, network access, or push credentials are needed. Run the restore and reset examples only in this scratch project.

Your working copy is the open change. Repository commands attempt snapshots; active hooks add capture attempts on supported events. Recovery depends on [coverage, capture success, and retention](concepts/snapshots-and-undo.md#coverage-and-limits).

## Get a repository

Open a clean Bash session so your usual prompt hooks and Git aliases do not add extra recovery steps to the transcript:

```sh
bash --noprofile --norc
```

In that shell, create a temporary directory and isolate the exercise from machine-specific Git settings, including signing. These environment variables apply only to this shell and its children; your configuration files are untouched.

```sh
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=fufu.updateCheck GIT_CONFIG_VALUE_0=false
unset FF_SESSION CLAUDE_CODE_SESSION_ID
SCENE=$(mktemp -d)
cd "$SCENE"
```

The `updateCheck` override disables release checks for this session so the exercise needs only its local remote.

Create the initial commit and the disposable remote. `command git` explicitly runs Git rather than a shell alias or function. The identities below are just labels for the exercise commits.

```sh
command git init -q -b main seed
command git -C seed config user.name 'Tutorial Reader'
command git -C seed config user.email reader@example.com
printf '# Tutorial project\n' > seed/README.md
command git -C seed add README.md
command git -C seed commit -qm 'docs: start the tutorial'
command git clone -q --bare seed origin.git
```

[`ff clone`](reference/cli/clone.md) creates your working checkout and initializes fufu in it:

```console
$ ff clone ./origin.git exercise
cloned into ./exercise — 1 commit on main
the net is on: ff undo has a floor to land on, and every verb takes one first
```

The second line means the operation log now has an earliest recovery point. [`ff undo`](reference/cli/undo.md) can restore retained local states recorded from this point onward. Enter the checkout and set its commit identity:

```sh
cd exercise
command git config user.name 'Tutorial Reader'
command git config user.email reader@example.com
```

For your own existing repository later, use [`ff init`](reference/cli/init.md); see [Adopting fufu](adopting.md). For shell and agent capture outside this controlled exercise, install and activate [`ff hook`](reference/cli/hook.md) integrations as described in [installation](install.md#install-hooks).

## Look around

Bare [`ff`](reference/cli/map.md) shows the map: recent commits and open or parked work across branches.

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
●  nszrlrzt 26306cdd   0s ago
   docs: start the tutorial
```

`@` is your open change; `no changes` means its files match the commit beneath it. `●` marks a commit recorded in branch history, and `▸ [main]` marks the current branch. The letters are a change ID; the hexadecimal value is a commit SHA. [Revisions and IDs](reference/revisions.md) explains how to use them. Your IDs, timestamps, and automatically generated branch name will differ from this run.

## Start work

<div class="demo cast" data-cast="../assets/tutorial/start-work.cast">
  <noscript><img src="../assets/tutorial/start-work.gif" alt="ff start creating a branch, the exact parser note, and ff status showing the open change"></noscript>
</div>

[`ff start`](reference/cli/switch.md) creates and switches to a new branch from trunk, which is `main` here. It chooses a branch name when you do not supply one:

```console
$ ff start
minted ff/noble-maple (forked from main)
switched to ff/noble-maple
undo: ff undo
```

Create `notes/parser.md` with these three lines:

```sh
mkdir notes
cat > notes/parser.md <<'EOF'
A char stream feeds the lexer.
The lexer emits spans.
Whitespace is the stream's problem, not the lexer's.
EOF
```

[`ff status`](reference/cli/status.md) captures eligible saved edits and shows what is uncommitted:

```console
$ ff status
on ff/noble-maple · nothing to pull
@  xqwkwqpz 44f0b294   0s ago
│  (no description)
│  A notes/parser.md +3  -0  ++++++++++++++++++++
│    1 file          +3  -0
●  nszrlrzt 26306cdd   0s ago
│  docs: start the tutorial
```

`A` means a new file: three added lines, none removed. No staging command is needed. [`ff diff`](reference/cli/diff.md) shows the same open change line by line, including eligible untracked files.

<a id="name-it-then-close-it"></a>

## Commit your changes

<div class="demo cast" data-cast="../assets/tutorial/name-it-then-close-it.cast">
  <noscript><img src="../assets/tutorial/name-it-then-close-it.gif" alt="ff describe setting a message, ff commit recording it, an exact second edit, and ff log"></noscript>
</div>

You can set a pending message with [`ff describe`](reference/cli/describe.md) while you work:

```console
$ ff describe -m "notes: parser skeleton and char stream"
pending description on ff/noble-maple: notes: parser skeleton and char stream
```

[`ff commit`](reference/cli/commit.md) records the edits in branch history, using that message:

```console
$ ff commit
closed 79fb3f38 on ff/noble-maple: notes: parser skeleton and char stream (1 file(s))
undo: ff undo
```

`closed` means the change is now recorded on your branch. A new open change is ready for the next edit. Append one line:

```sh
printf 'The lexer never sees whitespace.\n' >> notes/parser.md
```

This time, give the message directly to `ff commit`:

```console
$ ff commit -m "notes: drop whitespace from the stream"
closed a6cf0b91 on ff/noble-maple: notes: drop whitespace from the stream (1 file(s))
undo: ff undo
```

[`ff log`](reference/cli/log.md) shows the current branch's commits beneath the open change. `-n 5` limits the commit rows:

```console
$ ff log -n 5
@  no changes
│  (no description)
●  utkxrvkw a6cf0b91   0s ago
│  notes: drop whitespace from the stream
●  xqwkwqpz 79fb3f38   0s ago
│  notes: parser skeleton and char stream
●  nszrlrzt 26306cdd   0s ago
│  docs: start the tutorial
```

Your two commits are above the initial project commit. The open change is empty again. Internal snapshot objects and recorded branch commits are distinct; [Working copy and commits](concepts/changes.md#internal-storage-and-branch-history) covers storage and signing details.

## Switch without stashing

<div class="demo cast" data-cast="../assets/tutorial/switch-without-stashing.cast">
  <noscript><img src="../assets/tutorial/switch-without-stashing.gif" alt="ff switch leaving an unfinished edit on its branch, resuming it, renaming the branch, and restoring one file"></noscript>
</div>

Append a temporary note to this exercise's `README.md`:

```sh
printf '\nstray note\n' >> README.md
```

[`ff switch`](reference/cli/switch.md) parks the unfinished edit with the branch you leave:

```console
$ ff switch main
parked the open change on ff/noble-maple (3da267ff)
switched to main
undo: ff undo
```

You are now on `main`, where that note is absent. The map shows it saved with your task branch:

```console
$ ff
@  no changes                  ▸ [main]
│  (no description)
│ ●  utkxrvkw a6cf0b91   0s ago  ▸ [ff/noble-maple]  (+ parked change, 1 file)
│ │  notes: drop whitespace from the stream
│ ●  xqwkwqpz 79fb3f38   0s ago
├─╯  notes: parser skeleton and char stream
●  nszrlrzt 26306cdd   0s ago
   docs: start the tutorial
```

Switch back, substituting the branch name your `ff start` printed for `ff/noble-maple`:

```console
$ ff switch ff/noble-maple
switched to ff/noble-maple
resumed the parked change (1 file(s))
undo: ff undo
```

The temporary note is back. Rename the branch to `parser-stream`; its saved work and pending message follow the rename:

```console
$ ff describe -b parser-stream
claimed ff/noble-maple as parser-stream
undo: ff undo
```

`claimed` is the output for renaming an automatically named branch. Now discard the temporary README edit with [`ff restore`](reference/cli/restore.md). Restore takes a pre-operation snapshot of eligible edits before replacing the file with its committed content; the earlier switch also recorded this note. This example affects only the scratch checkout.

```console
$ ff restore README.md
restored from a6cf0b91 (notes: drop whitespace from the stream)
  restored  README.md
undo: ff undo
```

## Fix an earlier commit

<div class="demo cast" data-cast="../assets/tutorial/fix-an-earlier-commit.cast">
  <noscript><img src="../assets/tutorial/fix-an-earlier-commit.gif" alt="Adding a heading and using ff absorb to include it in the first task commit"></noscript>
</div>

The note needs a heading. Add it at the top, leaving the four existing lines intact:

```sh
printf '# Parser notes\n' | cat - notes/parser.md > notes/parser.md.new
mv notes/parser.md.new notes/parser.md
```

The heading belongs in the first task commit. [`ff absorb`](reference/cli/absorb.md) adds your open edits to that commit; `HEAD~1` selects the commit immediately before the current branch tip:

```console
$ ff absorb --into HEAD~1
moved 1 file(s) from the open change into 00df0275: notes: parser skeleton and char stream
restacked 1 commit(s) above it
undo: ff undo
```

The first commit now includes the heading, and the second was replayed above it. Your files already contain the result, so the working copy does not need another edit. The rewritten commits have new SHAs and keep their change IDs. [Rewriting history](guides/rewriting-history.md) covers other ways to move edits between commits.

<a id="line-up-then-send"></a>

## Pull updates and push your branch

<div class="demo cast" data-cast="../assets/tutorial/line-up-then-send.cast">
  <noscript><img src="../assets/tutorial/line-up-then-send.gif" alt="Creating a teammate update in the local remote, pulling it, and pushing parser-stream"></noscript>
</div>

Simulate a teammate updating `main` from a second clone. Run these commands from `exercise`, where you already are. They write only to the sibling scratch directories and the disposable local remote:

```sh
command git clone -q ../origin.git ../teammate
command git -C ../teammate config user.name 'Tutorial Teammate'
command git -C ../teammate config user.email teammate@example.com
printf 'A line from a teammate.\n' >> ../teammate/README.md
command git -C ../teammate commit -qam 'docs: a line from a teammate'
command git -C ../teammate push -q origin main
```

[`ff pull`](reference/cli/pull.md) updates your current branch from its base and remote copy. Here it will:

1. Fetch the teammate's commit from `origin`.
2. Advance local `main` to `origin/main`.
3. Replay your two task commits onto the updated `main` and update your working files.

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

Your README now includes the teammate's line, and the parser note still has all five lines. `not published yet` means your task branch has no remote copy. The indented `main` report describes the base branch update. Pull sends no branch updates to the remote; its local branch/file changes are undoable, while fetched objects and tracking refs are separate.

[`ff push`](reference/cli/push.md) sends `parser-stream`:

```console
$ ff push
created origin/parser-stream and set parser-stream to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease
```

Here `origin` is a directory on the same machine. The standard warning still applies: undo does not change the remote repository, even a local one. Your branch now tracks `origin/parser-stream`.

A lease checks that the remote ref still has the expected value before updating it. Replacing commits also checks fufu's seen record; fast-forwards can proceed without that agreement. A lease does not check ownership, and fufu has no special force-push protection for `main`. For real shared repositories, follow your team's history policy and server-side branch protection. [Pulling and pushing](concepts/push-boundary.md) covers leases and rollback.

<a id="undo-anything"></a>

## Undo a local operation

<div class="demo cast" data-cast="../assets/tutorial/undo-anything.cast">
  <noscript><img src="../assets/tutorial/undo-anything.gif" alt="Resetting the scratch branch by two commits and using ff undo to restore its recorded state"></noscript>
</div>

The preceding fufu commands recorded this branch and its files. With no new edits since that capture, use raw Git to reset the scratch branch by two commits:

```console
$ command git reset --hard HEAD~2
HEAD is now at 048cb72 docs: a line from a teammate
```

The parser note disappears from your working files. This command bypasses a shell Git alias; recovery here relies on the **preceding fufu captures**, not on Git taking a fresh snapshot. One `ff undo` restores the recorded local branch and files:

```console
$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/parser-stream moved to 048cb725 (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at 30fc5ecee521 (pushed parser-stream to origin/parser-stream)
  refs/heads/parser-stream → d1e64ac0
  1 worktree file(s) restored
back: ff redo
```

fufu observed the outside branch movement, then undid it. The parser note and your two task commits are back. The remote copy was unaffected by both the reset and undo. [`ff redo`](reference/cli/redo.md) would reapply the local reset.

Unsaved buffers, edits made after the last successful capture, ignored untracked files, and oversized working-copy content may not be recoverable. Retention also limits recovery, and undo follows this worktree's chain. See [snapshot coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits).

[`ff history`](reference/cli/history.md) shows available undo steps. `@` is where you are now; `↑1` is one redo ahead, and `↓1` is one undo back:

```console
$ ff history
↑1  5a5c7bd27eed    0s ago  redo  absorbed 1 foreign ref change(s)
@   30fc5ecee521    0s ago  now   pushed parser-stream to origin/parser-stream
↓1  60ca6b5a2c88    0s ago  undo  move from the open change into 79fb3f38 on parser-stream
↓2  516558286c11    0s ago  undo  pre: ff absorb --into HEAD~1
↓3  00c3b577dce0    0s ago  undo  claim ff/noble-maple as parser-stream
↓4  55ec0195d90f    0s ago  undo  switch from main to ff/noble-maple
↓5  b87ac2649f9b    0s ago  undo  switch from ff/noble-maple to main
↓6  ffb2c921b635    0s ago  undo  pre: ff switch main
↓7  ab24d91a382e    0s ago  undo  commit on ff/noble-maple: notes: drop whitespace from the stream
↓8  ad4cc6f80c64    0s ago  undo  pre: ff commit -m notes: drop whitespace from the stream
↓9  2e47a4a328fe    0s ago  undo  commit on ff/noble-maple: notes: parser skeleton and char stream
↓10 bb9b4485193d    0s ago  undo  describe pending change on ff/noble-maple
↓11 f8928822abfd    0s ago  undo  pre: ff status
↓12 7ecba9417742    0s ago  undo  mint ff/noble-maple at 26306cdd and switch from main
↓13 54335bdf0956    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)
```

The hexadecimal values here are operation IDs, not commit SHAs. `the floor` labels the earliest recovery point. [`ff op show`](reference/cli/op-show.md) inspects an operation; [`ff op restore`](reference/cli/op-restore.md) returns directly to a retained operation state. [Recovery](guides/recovery.md) explains how to choose a recovery command.

## Where you are now

You have created a branch, recorded commits, parked and resumed an edit, amended an earlier commit, pulled an update, pushed your branch, and recovered a local reset. All the exercise repositories are under the temporary directory in `$SCENE`; exit this Bash session when finished to return to your normal environment.

From here:

- [Adopting fufu](adopting.md) — use it in an existing repository.
- [Working copy and commits](concepts/changes.md) and [snapshots and undo](concepts/snapshots-and-undo.md) — understand what was recorded.
- [The command table](comparisons/command-table.md) — find the fufu command for a familiar Git task.
- [Plain-Git teammates](guides/plain-git-teammates.md) — collaborate through the same repository and remotes.
