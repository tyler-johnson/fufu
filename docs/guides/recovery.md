# Recovery

Choose recovery by what needs to move. Start in the worktree where the unwanted operation happened.

| Situation | Command | Effect and next step |
| --- | --- | --- |
| The latest local operation was wrong | [`ff undo`](../reference/cli/undo.md) | Restore files and local branch state one undo step back; inspect the result. |
| You undid too far | [`ff redo`](../reference/cli/redo.md) | Move forward on the available redo path. |
| One file needs its committed version | [`ff restore app.txt`](../reference/cli/restore.md) | Write that file from the commit under the open change; review the diff. |
| Files need a retained earlier version | `ff restore app.txt --at-op <operation>` or `ff restore --all --at 20m` | Write files only; HEAD, branches, and index stay where they are. |
| The whole local state needs an earlier recovery point | [`ff op restore <operation>`](../reference/cli/op-restore.md) | Move this worktree's recorded local state to that point; inspect files and branches. |
| An older ref change was wrong, but later unrelated work must remain | [`ff op revert <operation>`](../reference/cli/op-revert.md) | Invert that operation's ref transitions if still applicable; files, index, and HEAD selection stay in place. |

Recovery requires a successful, retained snapshot. Commands and active hooks capture at invocation boundaries, not continuously. Ignored untracked files, unsaved buffers, and working-copy content above `fufu.maxFileSize` (50 MiB by default) are excluded; oversized tracked files can retain older index/base content. Undo follows this worktree's chain and cannot reverse a remote update. Keep these [coverage and scope limits](../concepts/snapshots-and-undo.md#coverage-and-limits) in mind when selecting a point.

Each recipe below starts independently in a scratch repository with `app.txt` containing `hello`, a `README.md`, and an empty `feature` branch based on `main`. No tutorial state is needed. From the fufu source checkout, `bash scripts/docs/recovery-transcript.sh` reproduces all examples; pass a recipe ID such as `file` to run just that fixture. The script supplies the initial commit and enables fufu before the displayed commands. IDs and dates in the transcripts come from that run; use the ones your commands print.

<a id="an-agent-ran-git-reset-hard"></a>
## Recover after a raw Git reset

Prerequisite: the desired branch state was captured before the reset. This example commits the work through [`ff commit`](../reference/cli/commit.md), then resets it with raw Git. It has no uncaptured edits to recover.

<!-- transcript:reset -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 61556d9c on feature: app: feature (1 file(s))
undo: ff undo

$ git reset --hard HEAD~1
HEAD is now at 897cc69 demo: initial files

$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/feature moved to 897cc698 (reset: moving to HEAD~1)
undid (a change made outside fufu): absorbed 1 ref change(s); previous op may not have completed
  now at 915a46d79f7a (commit on feature: app: feature)
  refs/heads/feature → 61556d9c
  1 worktree file(s) restored
back: ff redo

```
<!-- /transcript -->

The next fufu command reconciles the foreign ref movement, then undo restores the retained branch tip and files. Check the files and [`ff status`](../reference/cli/status.md) before continuing. Raw Git only gets a fresh pre-command capture when an active hook or shell alias invokes fufu first; otherwise recovery reaches the last retained state. See [using fufu alongside Git](../concepts/two-regimes.md).

The `previous op may not have completed` wording is the current reconciliation report in this fixture: the raw reset moved a ref away from the state recorded by the completed commit. The source check verifies that undo returns to that commit and restores its file.

<a id="i-want-one-file-back-the-way-it-was"></a>
## Restore one file

Prerequisite: `app.txt` is tracked and its committed version is the desired source. Bare restore discards its working-copy edits. The example first labels a snapshot with [`ff trigger`](../reference/cli/trigger.md), so it can also demonstrate retrieving the discarded edit by its exact operation ID.

<!-- transcript:file -->
```console
$ printf 'wrong\n' > app.txt

$ ff trigger -m "before discarding the edit"
b177304ea262 · 1 file

$ ff restore app.txt
restored from 897cc698 (demo: initial files)
  restored  app.txt
undo: ff undo

$ ff restore app.txt --at-op b177304ea262
restored from b177 (manual: before discarding the edit)
  restored  app.txt
undo: ff undo

$ ff restore app.txt --from main
restored from 897cc698 (demo: initial files)
  restored  app.txt
undo: ff undo

```
<!-- /transcript -->

`--from main` selects a commit source instead. All three restores leave branches, HEAD, and the index unchanged. Restore requires its own successful pre-restore capture before writing files. For precise recovery of a discarded edit, find that capture with [`ff op log`](../reference/cli/op-log.md) and use `--at-op`; undo groups consecutive captures and can step past the individual file state you want. Review with [`ff diff`](../reference/cli/diff.md), then commit or keep editing.

## Restore files by time or snapshot

Prerequisite: the desired files were captured at or before the requested time and that operation is retained. Here a named draft capture is followed by a bad commit. The timestamp and pause make the two states distinguishable in this short example.

<!-- transcript:past-files -->
```console
$ printf 'keep this draft\n' > app.txt

$ ff trigger -m "known good draft"
63a3306927f3 · 1 file

$ saved_time=$(date -u +%Y-%m-%dT%H:%M:%SZ)

$ sleep 2

$ printf 'bad refactor\n' > app.txt

$ ff commit -m "app: bad refactor"
closed 3e776984 on feature: app: bad refactor (1 file(s))
undo: ff undo

$ ff restore --all --at "$saved_time"
restored from 63a3 (manual: known good draft)
  restored  app.txt
undo: ff undo

$ ff restore app.txt --at-op 63a3306927f3
restored from 63a3 (manual: known good draft)
  (no files differed)
undo: ff undo

```
<!-- /transcript -->

The files become the earlier draft, while the bad commit remains at HEAD and the index stays unchanged. Review and commit the correction if you want to keep later history. `--at 20m`, `2h`, or a date selects the operation current at that time, not continuous file history. Use `--at-op` when you know the exact point. [Revisions and IDs](../reference/revisions.md#paths-sources-and-past-state-reads) defines these sources.

<a id="i-want-the-whole-tree-from-twenty-minutes-ago"></a>
## Restore files and branch state together

Prerequisite: you want to return local state to an earlier operation, including undoing later branch movement. [`ff history`](../reference/cli/history.md) shows this worktree's undo steps; its hexadecimal IDs address operations. Inspect the chosen record with [`ff op show`](../reference/cli/op-show.md) before restoring it.

<!-- transcript:state -->
```console
$ printf 'good\n' > app.txt

$ ff commit -m "app: good version"
closed dd0fd0cd on feature: app: good version (1 file(s))
undo: ff undo

$ printf 'bad\n' > app.txt

$ ff commit -m "app: wrong direction"
closed 685d7f03 on feature: app: wrong direction (1 file(s))
undo: ff undo

$ ff history
@   e0153c819a24    0s ago  now   commit on feature: app: wrong direction
↓1  b016545cb163    0s ago  undo  pre: ff commit -m app: wrong direction
↓2  cb1d625edebc    0s ago  undo  commit on feature: app: good version
↓3  89329a7eff0b    0s ago  undo  pre: ff commit -m app: good version
↓4  392d9dd5227c    0s ago  undo  mint feature at 6eabd5cd and switch from main
↓5  6fa8830fc477    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)

$ ff op show cb1d625edebc
cb1d625edebc  op  0s ago
  commit on feature: app: good version
  on        feature
  base      6eabd5cd
  refs/heads/feature → dd0fd0cd
  (the worktree is unchanged across it)

$ ff op restore cb1d625edebc
undid: commit on feature: app: wrong direction
  now at cb1d625edebc (commit on feature: app: good version)
  refs/heads/feature → dd0fd0cd
  1 worktree file(s) restored
back: ff redo

```
<!-- /transcript -->

The branch and file both return to the good commit. Worktree ownership guards still apply; this does not restore every checkout in the repository at once. Inspect status and history afterward. Use a file restore instead when later branch history should stay.

<a id="i-undid-too-far"></a>
## Redo, or recover a forked-off operation

Prerequisite: a recent local operation was undone. Redo works until new work creates a different continuation. This recipe first redoes successfully, then records a different message after undoing again.

<!-- transcript:redo -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 271aa7ca on feature: app: feature (1 file(s))
undo: ff undo

$ ff undo
undid: commit on feature: app: feature
  now at e466d7ae9876 (pre: ff commit -m app: feature)
  refs/heads/feature → 6eabd5cd
back: ff redo

$ ff redo
redid: commit on feature: app: feature
  now at 8998b750f354 (commit on feature: app: feature)
  refs/heads/feature → 271aa7ca
back: ff undo

$ ff undo
undid: commit on feature: app: feature
  now at e466d7ae9876 (pre: ff commit -m app: feature)
  refs/heads/feature → 6eabd5cd
back: ff redo

$ ff commit -m "app: better message"
closed 9d098532 on feature: app: better message (1 file(s))
undo: ff undo

$ ff redo
ff: nothing to redo: work has landed since the last undo, so the log forked rather than rewound
  try:
    ff op log
    ff history

$ ff op log --at-op 8998b750f354 -n 3
8998b750f354   0s ago  op      feature       commit on feature: app: feature
e466d7ae9876   0s ago  capture feature       pre: ff commit -m app: feature
392d9dd5227c   0s ago  op      feature       mint feature at 6eabd5cd and switch from main

$ ff op restore 8998b750f354
undid: commit on feature: app: better message
  now at 8998b750f354 (commit on feature: app: feature)
  2 operations stepped over
  refs/heads/feature → 271aa7ca
back: ff redo

```
<!-- /transcript -->

The refusal belongs to the second redo: the new commit forked the operation log. The old operation remains addressable while retained. Bare op log lists the live chain; `--at-op` inspects the saved old point and its predecessors. Op restore uses that same old ID to return to it. Inspect the result before adding more work; [retention](#retention-and-the-earliest-recovery-point) bounds how long these points remain available.

<a id="two-writers-on-one-chain-and-only-one-was-wrong"></a>
## Reverse an older ref change after unrelated work

Prerequisite: the unwanted operation moved refs, those refs still equal the values it left, and the later work is on other refs. The example commits on `feature`, then uses [`ff switch`](../reference/cli/switch.md) to create `docs` from main and commits useful work there.

<!-- transcript:revert -->
```console
$ printf 'wrong branch work\n' > app.txt

$ ff commit -m "app: unwanted commit"
closed 2178bc88 on feature: app: unwanted commit (1 file(s))
undo: ff undo

$ ff switch main -b docs
minted docs (forked from main)
switched to docs
undo: ff undo

$ printf 'useful later work\n' > notes.txt

$ ff commit -m "docs: useful notes"
closed 5fe2dacc on docs: docs: useful notes (1 file(s))
undo: ff undo

$ ff op log -n 6
4ca81a2f6710   0s ago  op      docs          commit on docs: docs: useful notes
47eb9bfbd483   0s ago  capture docs          pre: ff commit -m docs: useful notes
211ba99c007e   0s ago  op      docs          mint docs at 6eabd5cd and switch from feature
2bda3463f5a9   0s ago  op      feature       commit on feature: app: unwanted commit
ffb05cbd1a35   0s ago  capture feature       pre: ff commit -m app: unwanted commit
392d9dd5227c   0s ago  op      feature       mint feature at 6eabd5cd and switch from main

$ ff op revert 2bda3463f5a9
reverted 2bda3463f5a9cc0db16e2f669a710b2f3eb67a13: commit on feature: app: unwanted commit
  refs/heads/feature → 6eabd5cd
undo: ff undo

$ ff undo
undid: revert 2bda3463 (commit on feature: app: unwanted commit)
  now at 4ca81a2f6710 (commit on docs: docs: useful notes)
  refs/heads/feature → 2178bc88
back: ff redo

```
<!-- /transcript -->

Revert moves `feature` back without moving `docs`, changing the checkout selection, or writing files or index. It records a new operation; the final undo reverses the revert. Inspect the affected branch with [`ff log`](../reference/cli/log.md) before continuing. Reverting a current branch's ref also leaves its files in place, so those files can now differ from HEAD.

### When op revert refuses

Prerequisite: later work moved the same ref. This independent counterexample shows the applicability check:

<!-- transcript:revert-refusal -->
```console
$ printf 'first\n' > app.txt

$ ff commit -m "app: first"
closed 29b1e91f on feature: app: first (1 file(s))
undo: ff undo

$ printf 'later\n' > app.txt

$ ff commit -m "app: later"
closed c1f070ec on feature: app: later (1 file(s))
undo: ff undo

$ ff op log -n 4
c84de588230e   0s ago  op      feature       commit on feature: app: later
d32b79740f3e   0s ago  capture feature       pre: ff commit -m app: later
25435854cfc5   0s ago  op      feature       commit on feature: app: first
4773f6b29e1d   0s ago  capture feature       pre: ff commit -m app: first

$ ff op revert 25435854cfc5
ff: inverting 25435854cfc5f958b4e7dc89b7973c6064cbabd1 conflicts with work done since; no ref inversion was applied: refs/heads/feature: the operation left it at 29b1e91f75bd2b09ec92558c019a1611cf13e1d5, and it now stands at c1f070ecc32116a680b03760d8ae0edbcc1e7085
  try:
    ff op show 25435854cfc5f958b4e7dc89b7973c6064cbabd1
    ff explain held/op-revert
    ff op log

```
<!-- /transcript -->

`held/op-revert` is an applicability refusal, not a new resolution session. The target ref transitions are not applied; pre-capture and reconciliation can still write records. Captures and notes have no invertible ref transition, and revert does not selectively reverse file patches or replay later same-branch commits. Use [history rewriting](rewriting-history.md#split-a-commit-that-already-closed) to change older content while preserving subsequent commits, or op restore if the whole later local state should be taken back.

<a id="i-committed-to-the-wrong-branch-or-with-the-wrong-message"></a>
## Move a just-made commit to a new branch

Prerequisite: the unwanted close is the latest operation, and `correct-branch` does not exist. Undo makes its work open again; a commit with `-b` records it on a new branch.

<!-- transcript:wrong-branch -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 71e12fbc on feature: app: feature (1 file(s))
undo: ff undo

$ ff undo
undid: commit on feature: app: feature
  now at b6c05ebee1c9 (pre: ff commit -m app: feature)
  refs/heads/feature → 6eabd5cd
back: ff redo

$ ff commit -b correct-branch -m "app: feature"
closed 71e12fbc on correct-branch: app: feature (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The original `feature` tip stays at its prior commit. Review the new branch, then continue there. For only a wrong message, use [`ff describe HEAD -m "new message"`](../reference/cli/describe.md); [rewriting history](rewriting-history.md) covers older commits and moving selected files.

## Undo an editing or resolution session

Prerequisite: you are editing an earlier commit or resolving a held rewrite. Check history to distinguish opening the session, switching into it, and finishing it.

- [`ff edit`](../reference/cli/edit.md) and [`ff resolve`](../reference/cli/resolve.md) open rewrite sessions through creation and switching: one undo returns to the original branch, another removes the session.
- [`ff done`](../reference/cli/done.md) or `ff done --abandon` lands or abandons in one operation. One undo brings that session back, including retained uncommitted edits.
- A held parked-change arrival resolves in place; it has no session or done step. Fix the files and continue ordinary work.

The [editing recipe](rewriting-history.md#reopen-a-closed-commit) and [conflict guide](../concepts/held-rewrites.md#abandoning-or-undoing-a-resolution) show the surrounding workflow. Session edits remain in operation history when abandoned; they are not a Git stash.

These independent examples verify both opening paths. The first opens an editing session and reverses its switch and creation:

<!-- transcript:session-undo -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 99d8d6b9 on feature: app: feature (1 file(s))
undo: ff undo

$ ff edit HEAD
editing 99d8d6b9 "app: feature" on ff/dusky-reef
finish with ff done, or ff done --abandon to drop it
undo: ff undo

$ ff history
@   8dee0084e385    0s ago  now   switch from feature to ff/dusky-reef
↓1  e61ecf2450a1    0s ago  undo  edit 99d8d6b9: session ff/dusky-reef on feature
↓2  789b60c3fe21    0s ago  undo  commit on feature: app: feature
↓3  e791a0a3e2df    0s ago  undo  pre: ff commit -m app: feature
↓4  392d9dd5227c    0s ago  undo  mint feature at 6eabd5cd and switch from main
↓5  6fa8830fc477    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)

$ ff undo
undid: switch from feature to ff/dusky-reef
  now at e61ecf2450a1 (edit 99d8d6b9: session ff/dusky-reef on feature)
  HEAD → refs/heads/feature
back: ff redo

$ ff undo
undid: edit 99d8d6b9: session ff/dusky-reef on feature
  now at 789b60c3fe21 (commit on feature: app: feature)
  refs/heads/ff/dusky-reef deleted
back: ff redo

```
<!-- /transcript -->

The second creates conflicting branch edits, opens a resolution, then reverses its switch and creation. The original held request remains after the two undos; resolve it again or use `ff resolve --abandon` if you no longer want that replay.

<!-- transcript:resolution-undo -->
```console
$ printf 'feature greeting\n' > app.txt

$ ff commit -m "app: feature greeting"
closed f1678cb7 on feature: app: feature greeting (1 file(s))
undo: ff undo

$ ff switch main
switched to main
undo: ff undo

$ printf 'main greeting\n' > app.txt

$ ff commit -m "app: main greeting"
closed 068b64c7 on main: app: main greeting (1 file(s))
undo: ff undo

$ ff switch feature
switched to feature
undo: ff undo

$ ff restack
held: replaying f1678cb7 "app: feature greeting" conflicts in app.txt
    the restack of 1 commit on feature is waiting — nothing was written
    ff resolve to fix them, all at once · ff resolve --abandon to drop it

$ ff resolve
resolving 1 conflict in app.txt on ff/warm-drake
    1 commits replayed
    fix the markers, then ff done · ff resolve --abandon to drop it

$ ff undo
undid: switch from feature to ff/warm-drake
  now at 3e644f7f640e (resolve the held restack on feature: 1 region(s))
  HEAD → refs/heads/feature
  1 worktree file(s) restored
back: ff redo

$ ff undo
undid: resolve the held restack on feature: 1 region(s)
  now at b1e33073fe10 (hold restack of feature onto main)
  refs/heads/ff/warm-drake deleted
back: ff redo

```
<!-- /transcript -->

<a id="someone-force-pushed-over-my-branch"></a>
## Recover from a changed remote branch

Prerequisite: your branch was published and someone changed its remote tip. This scratch recipe creates a same-machine `origin.git` and a plain-Git checkout at `../teammate`. The displayed teammate commands amend and force-push only that disposable remote.

<!-- transcript:force-push -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 5401b8e0 on feature: app: feature (1 file(s))
undo: ff undo

$ ff push
created origin/feature and set feature to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

$ git -C ../teammate fetch -q origin

$ git -C ../teammate switch -q feature

$ git -C ../teammate commit -q --amend -m "app: reviewed feature"

$ git -C ../teammate push -q --force origin feature

$ printf 'local follow-up\n' > notes.txt

$ ff commit -m "docs: follow-up"
closed ae08a41d on feature: docs: follow-up (1 file(s))
undo: ff undo

$ ff push
ff: origin/feature moved since you last looked, so nothing was pushed — your commits are still here, and ff pull takes in what arrived
  try:
    ff pull feature
    ff push feature

$ ff pull
fetching from origin
took in 1 commit(s) from origin/feature
replayed 1 of yours on top
1 commit(s) to push — ff push
undo: ff undo

$ ff push
pushed feature to origin/feature
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

```
<!-- /transcript -->

The first retry of [`ff push`](../reference/cli/push.md) refuses the stale lease; local work remains. [`ff pull`](../reference/cli/pull.md) fetches the changed remote history and replays the local follow-up. This raw Git amend removes the stored change-ID header, so no identity-based superseded line appears. Inspect and test the result, then push under the refreshed lease. A pull conflict needs [resolution](../concepts/held-rewrites.md) first.

Pull's local branch and file updates are undoable; fetching objects and tracking refs is separate. A push changes the remote and is outside undo's reach. Rolling it back requires another allowed push, and cannot reverse other clones, CI runs, or webhooks. Leases check the remote ref value, not ownership or a special rule for main; [pulling and pushing](../concepts/push-boundary.md) explains seen records and team policy.

<a id="i-need-a-parked-change-back-with-plain-git"></a>
## Recover parked work without fufu

Prerequisite: a parked change still exists at `refs/fufu/open/<branch>`. With fufu, switch to its branch to resume it. Without fufu, the [plain-Git recovery recipe](plain-git-teammates.md#recover-parked-work-with-git) shows how to inspect that exact ref and apply its patch as uncommitted work, avoiding unrelated operation records in `git log --all`.

<a id="what-undo-cannot-reach"></a>
## Retention and the earliest recovery point

Undo cannot reach before fufu's first recorded state, into another worktree's chain, or into uncaptured or expired file states. Raw file changes that move no refs can disappear before any capture sees them. [Snapshot coverage](../concepts/snapshots-and-undo.md#coverage-and-limits) applies to every recipe above.

[`ff op trim`](../reference/cli/op-trim.md) removes old operations according to `fufu.keep` (90 days by default). Automatic trimming normally runs at most daily per worktree. For a real repository, choose a window such as [`ff config keep 30d`](../reference/cli/config.md) and preview with `ff op trim -n`.

The following `retention` recipe deliberately uses seconds in its disposable repository to show the effect. Do not use its short window for everyday work.

<!-- transcript:retention -->
```console
$ printf 'retained in branch history\n' > app.txt

$ ff commit -m "app: keep this commit"
closed ea606571 on feature: app: keep this commit (1 file(s))
undo: ff undo

$ ff config keep 2s
keep = 2s (this repo)

$ sleep 3

$ ff switch main
switched to main
undo: ff undo

$ ff switch feature
switched to feature
undo: ff undo

$ ff op trim -n
would drop 4 of 6 operations

$ ff op trim
dropped 4 of 6 operations — previous tip saved at refs/fufu/wt/main/trash/@ops until the next trim
dropped data frees after gc

$ ff history
@   2397f30f448e    0s ago  now   trim: dropped 4 operation(s)
↓1  52efad6d8630    0s ago  undo  switch from feature to main
    (the floor)

$ ff config keep 90d
keep = 90d (this repo)

```
<!-- /transcript -->

Branch commits and files are unchanged; the earliest undo point moves forward. A real trim saves the previous chain tip at `refs/fufu/wt/<worktree>/trash/@ops` until the next trim and invokes Git's automatic garbage collection. A dry run does not drop operations, but capture and update maintenance can still run. Surviving operation IDs can change when predecessor links are rewritten. Restore or commit important work before its retention window expires, including work from [removed worktrees](worktrees.md#removal-captures-first).
