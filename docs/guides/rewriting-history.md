# Rewriting history

Use these recipes to change local commit history. Select the task first:

| Task | Command | Default or result |
| --- | --- | --- |
| Reword a recorded commit | [`ff describe HEAD -m "message"`](../reference/cli/describe.md) | Change the latest commit's message; bare describe sets the open change's pending message. |
| Amend with current edits | [`ff absorb`](../reference/cli/absorb.md) | Move open work into the commit under it; `--into` selects another target. |
| Combine commits | `ff absorb --from HEAD~2..HEAD` | Fold the last two commits into the one below them. |
| Split current work | [`ff commit app.txt -m "message"`](../reference/cli/commit.md) | Record selected paths; other files stay open. |
| Split a recorded commit | [`ff lift notes.txt`](../reference/cli/lift.md) | Move selected files from the latest commit back into open work. |
| Edit an earlier file version | [`ff edit HEAD~1`](../reference/cli/edit.md), then [`ff done`](../reference/cli/done.md) | Open a session at that commit, apply edits, replay later commits, and return. |

Each recipe starts independently on an empty `feature` branch based on main, with `app.txt` containing `hello` and a `README.md`. `bash scripts/docs/rewriting-history-transcript.sh` creates the scratch fixtures and checks their results; pass a recipe ID such as `split` to run one. The scripts supply the seed commit and fufu initialization; all task edits are displayed below.

Rewrites preserve the change IDs of surviving changes while creating new commit hashes. [`ff log`](../reference/cli/log.md) displays the stable k–z change ID before the hexadecimal commit hash. Use [Revisions and IDs](../reference/revisions.md) for selection syntax and prefix rules.

## Reword a closed commit

Prerequisite: the commit exists in local history and only its message needs changing. Name `HEAD` for the latest commit or an earlier revision instead.

<!-- transcript:reword -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m wip
closed 3ca76bdc on feature: wip (1 file(s))
undo: ff undo

$ ff describe HEAD -m "app: greet the reader"
reworded f787eacb on feature: app: greet the reader
undo: ff undo

```
<!-- /transcript -->

The file tree and change identity stay the same; the commit hash changes. Later commits are replayed automatically and receive new hashes too. Check log, then continue editing or [send the rewrite](#rewriting-pushed-work) when appropriate.

## Fold the open change into a closed commit

Prerequisite: feedback is already written in your working copy. Here `app.txt` belongs in the latest commit and `notes.txt` should remain separate.

<!-- transcript:amend -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed a4e8e4f4 on feature: app: feature (1 file(s))
undo: ff undo

$ printf 'reviewed feature\n' > app.txt

$ printf 'unrelated note\n' > notes.txt

$ ff absorb app.txt
moved 1 file(s) from the open change into 95f2c487: app: feature
limited to 1 path(s)
the rest of your change is still open
undo: ff undo

$ ff status
on feature · nothing to pull
@  zulqpnow 2e92802d   0s ago
│  (no description)
│  A notes.txt +1  -0  ++++++++++++++++++++
│    1 file    +1  -0
●  ovootzkz 95f2c487   0s ago
│  app: feature

```
<!-- /transcript -->

Only the selected file's change moves; notes remain open for a separate commit. To target an earlier commit, use `ff absorb app.txt --into HEAD~1`. Omit the path to move all eligible open work. Review status afterward and commit the remainder when ready. Adding only uncommitted work to trunk's tip is also allowed.

### Paths and hunk limits

Commit, absorb, and lift accept files or directory prefixes, not globs, exclusions, or an interactive hunk picker. Selection moves a path's change as a whole. Fufu does not infer which commit owns each hunk.

For two edits in one file, save the full version with [`ff trigger`](../reference/cli/trigger.md), arrange the file into the first desired version, commit it, then use [`ff restore`](../reference/cli/restore.md) to recover the full version. This independent recipe uses two lines so each resulting commit is easy to inspect:

<!-- transcript:same-file -->
```console
$ printf 'first change\nsecond change\n' > app.txt

$ ff trigger -m "complete two-part edit"
1a4efe152e4b · 1 file

$ printf 'first change\n' > app.txt

$ ff commit -m "app: first change"
closed e4848d67 on feature: app: first change (1 file(s))
undo: ff undo

$ ff restore app.txt --at-op 1a4efe152e4b
restored from 1a4e (manual: complete two-part edit)
  restored  app.txt
undo: ff undo

$ ff commit -m "app: second change"
closed cc3daf99 on feature: app: second change (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The second commit contains only the difference between the first version and the restored full version. Review each with [`ff show`](../reference/cli/show.md). [File recovery](recovery.md#restore-one-file) explains capture limits and choosing the right retained snapshot.

## Split at the close

Prerequisite: several files are open and should become separate commits. No staging is required.

<!-- transcript:partial -->
```console
$ printf 'feature\n' > app.txt

$ printf 'documentation\n' > notes.txt

$ ff commit app.txt -m "app: feature"
closed a624ba49 on feature: app: feature (1 file(s))
undo: ff undo

$ ff status
on feature · nothing to pull
@  tnyvmwuq ab1a4078   0s ago
│  (no description)
│  A notes.txt +1  -0  ++++++++++++++++++++
│    1 file    +1  -0
●  xqslqzqu a624ba49   0s ago
│  app: feature

$ ff commit -m "docs: feature notes"
closed 156ff3f2 on feature: docs: feature notes (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The first commit contains only `app.txt`; the second records the remaining notes. Inspect each commit with ff show. Repeat path-limited commits to build more layers.

## Split a commit that already closed

Prerequisite: the latest commit mixes files that belong apart. Lift defaults to taking work from HEAD into the open change; `--from` names an earlier source.

<!-- transcript:split -->
```console
$ printf 'feature\n' > app.txt

$ printf 'documentation\n' > notes.txt

$ ff commit -m "app: feature and notes"
closed 23e782c1 on feature: app: feature and notes (2 file(s))
undo: ff undo

$ ff lift notes.txt
moved 1 file(s) from 23e782c1 "app: feature and notes" into the open change
limited to 1 path(s)
undo: ff undo

$ ff commit -m "docs: feature notes"
closed c6ddcd16 on feature: docs: feature notes (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The original commit keeps its change ID and gets a new hash with one file's change removed. Notes become open work, then a separate commit. Review both commits; describe the remaining original commit if its old message is now too broad. A lift with no paths takes everything and drops the source if it becomes empty. [`ff undo`](../reference/cli/undo.md) reverses a successful lift and its replay in one step.

## Combining commits

Prerequisite: the last three commits are one logical change. Preview the bounded source range before combining its top two commits into the bottom one.

<!-- transcript:combine -->
```console
$ printf 'number\n' > app.txt

$ ff commit -m "app: numbers"
closed 5addf8d9 on feature: app: numbers (1 file(s))
undo: ff undo

$ printf 'signed\n' >> app.txt

$ ff commit -m "app: signed numbers"
closed b9c8ace4 on feature: app: signed numbers (1 file(s))
undo: ff undo

$ printf 'float\n' >> app.txt

$ ff commit -m "app: floats"
closed e1dfd566 on feature: app: floats (1 file(s))
undo: ff undo

$ ff log -r HEAD~2..HEAD
●  wlyvmwvl e1dfd566   0s ago
│  app: floats
●  wmtvrqmo b9c8ace4   0s ago
│  app: signed numbers

$ ff absorb --from HEAD~2..HEAD -m "app: signed and floating numbers"
moved 1 file(s) from 2 commits (b9c8ace4..e1dfd566) into 32d84df4: app: signed and floating numbers
dropped 2 commit(s) that change nothing: b9c8ace4, e1dfd566
undo: ff undo

```
<!-- /transcript -->

The result has the same final files and one combined commit. Source commits emptied by the move are dropped. Keep both endpoints: unbounded `x..` ranges over visible history, not just the current branch. Sources must form a contiguous run; see [range endpoints](../reference/revisions.md#set-operators-and-range-endpoints). When committed sources would default to a target on trunk's history, absorb refuses; name the intended target explicitly with `--into`. Review the result in log and run the relevant project tests.

### Re-split a range

Prerequisite: the last two commits need a different grouping. Lift can reopen that entire bounded range, then path-limited commits record the replacement grouping.

<!-- transcript:lift-range -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 48af924a on feature: app: feature (1 file(s))
undo: ff undo

$ printf 'documentation\n' > notes.txt

$ ff commit -m "docs: feature notes"
closed 485f3e58 on feature: docs: feature notes (1 file(s))
undo: ff undo

$ ff lift --from HEAD~2..HEAD
moved 2 file(s) from 2 commits (48af924a..485f3e58) into the open change
dropped 2 commit(s) that change nothing: 48af924a, 485f3e58
undo: ff undo

$ ff commit app.txt -m "app: feature"
closed 41a66e6b on feature: app: feature (1 file(s))
undo: ff undo

$ ff commit notes.txt -m "docs: feature notes"
closed b27ec2ec on feature: docs: feature notes (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The lifted commits disappear from branch history while their files remain open. The subsequent commits record the selected layers. Review each before moving on.

## Reopen a closed commit

Prerequisite: you need the earlier commit's actual files on disk to make or test a fix. This example keeps later documentation in a separate commit, then edits its parent.

<!-- transcript:edit -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed dce4cfe0 on feature: app: feature (1 file(s))
undo: ff undo

$ printf 'documentation\n' > notes.txt

$ ff commit -m "docs: feature notes"
closed 9c3b2d8e on feature: docs: feature notes (1 file(s))
undo: ff undo

$ ff edit HEAD~1
editing dce4cfe0 "app: feature" on ff/rosy-lynx
1 commit(s) wait ahead on feature
finish with ff done, or ff done --abandon to drop it
undo: ff undo

$ printf 'reviewed feature\n' > app.txt

$ ff done
amended dce4cfe0 "app: feature"
replayed 1 commit(s)
back on feature
undo: ff undo

```
<!-- /transcript -->

Edit creates a temporary session branch and switches into it. Original open work parks with the branch you left. Done amends the selected commit, replays later commits, and returns to that branch with its parked work resumed. Review and test the replayed result. `ff edit main` is a branch switch; use a commit revision when you want an editing session.

### Abandon an experiment

Prerequisite: an editing session contains changes you do not want to apply. Abandon returns without changing the original history. The final undo below demonstrates recovery of the abandoned session.

<!-- transcript:abandon -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed c745f588 on feature: app: feature (1 file(s))
undo: ff undo

$ ff edit HEAD
editing c745f588 "app: feature" on ff/lively-coral
finish with ff done, or ff done --abandon to drop it
undo: ff undo

$ printf 'experiment\n' > app.txt

$ ff done --abandon
abandoned the session on c745f588 "app: feature"
the session's edits stay at 7a176a54; ff undo brings the session back
back on feature
undo: ff undo

$ ff undo
undid: done --abandon: c745f588 on feature
  now at e48fa82f0ac1 (pre: ff done --abandon)
  refs/heads/ff/lively-coral → c745f588
  HEAD → refs/heads/ff/lively-coral
  1 worktree file(s) restored
back: ff redo

```
<!-- /transcript -->

The edits remain in retained operation history, not a Git stash. After undo, inspect the restored session and either finish it or abandon it again. Opening an edit or rewrite-resolution session takes two undo steps to reverse fully: switch back, then remove the session. Finishing or abandoning takes one. [Recovery](recovery.md#undo-an-editing-or-resolution-session) explains the scope.

## Conflicts and dependent branches

A conflicting primary replay leaves its branch tip and files at their pre-replay state and records a [held rewrite](../concepts/held-rewrites.md). Capture and metadata may still be written. A downstream conflict can occur after earlier branches have already updated. Read the whole report, switch to the held branch, run [`ff resolve`](../reference/cli/resolve.md), fix the markers, and finish with done.

Successful rewrites and their dependent-branch replay are one undoable operation. Exit behavior varies when only a downstream branch holds; [the cascade table](stacked-changes.md#the-cascade) lists the current exceptions. A whole editing or resolution session spans several operations.

<a id="would-two-branches-collide"></a>
## Check branch overlap

The independent [collision recipe](stacked-changes.md#would-two-branches-collide) now lives with branch coordination. It checks conflicts, lifts one overlapping file, and checks again after discarding that lifted edit.

<a id="trim-the-operation-log"></a>
## Retain recovery points

The [retention recipe](recovery.md#retention-and-the-earliest-recovery-point) now lives in recovery. Trimming limits operation history, including abandoned edits; it does not remove commits from branch history.

## Signed commits are re-signed

When `commit.gpgsign` is enabled, rewritten history commits are signed again, including replays. Otherwise an old signature does not carry over. [Commit signing](../reference/signing.md) owns configuration, signer costs, and the distinction between history commits and unsigned internal objects.

<a id="the-append-only-boundary"></a>
## Rewriting pushed work

Prerequisite: the team permits rewriting the branch and the server allows it. Rewrite verbs accept pushed commits; they only change local history. This recipe uses a disposable same-machine origin.

<!-- transcript:published -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 26ab1d75 on feature: app: feature (1 file(s))
undo: ff undo

$ ff push
created origin/feature and set feature to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

$ ff describe HEAD -m "app: reviewed feature"
reworded ee4d2629 on feature: app: reviewed feature
1 of the rewritten commits are already on origin/feature
undo: ff undo

$ ff push
pushed feature to origin/feature
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

```
<!-- /transcript -->

The first [`ff push`](../reference/cli/push.md) publishes the original commit. Describe reports that it rewrote published work but sends nothing. The second push sends the new commit under a lease: the remote ref must match the expected value, and replacing commits also requires agreement between the seen record and tracking tip. Fast-forwards have a seen-record exception. There is no ownership check or special protection for main; team policy and server-side branch protection decide what may be sent.

If the remote moved, [pull and inspect the reconciliation](recovery.md#someone-force-pushed-over-my-branch) before retrying. [`ff pull`](../reference/cli/pull.md) can drop a local version when the target contains the same change ID, even if their content differs. A held branch cannot be pushed until its hold is resolved or abandoned. Undo reaches local state; a remote rollback requires another permitted push. [Pulling and pushing](../concepts/push-boundary.md) has the complete contract.

## Where next

- [Stacked changes](stacked-changes.md) — apply feedback across dependent branches and move remaining work after a merge.
- [Recovery](recovery.md) — choose between undo, file restore, whole-state restore, and ref revert.
- [Conflicts and held rewrites](../concepts/held-rewrites.md) — finish a replay that needs manual resolution.
