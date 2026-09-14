# Worktrees

Use a worktree when you need two simultaneous checkouts: a build running while you edit elsewhere, an independent agent task, or a review checkout. For sequential work, [`ff switch`](../reference/cli/switch.md) already parks uncommitted work with its branch and resumes it on return.

[`ff worktree`](../reference/cli/worktree.md) creates another checkout sharing this repository's Git objects and refs. Each worktree has its own files, index, HEAD, operation chain, and lock. Bare worktree lists live checkouts and retained chains from removed checkouts.

Every recipe starts independently in an initialized scratch repository on `feature`, based on main, with `app.txt` containing `hello` and a `README.md`. `bash scripts/docs/worktrees-transcript.sh` runs all recipes and checks the recovered files; pass an ID such as `removal` for one. Every command targeting the second checkout uses `ff -C ../review`; paths in output are the real scratch paths from the recorded run.

<a id="a-second-checkout"></a>
<a id="each-tree-has-its-own-open-change"></a>
## Create a checkout and work independently

Prerequisite: `../review` does not exist and the branch name `review` is available. Without an explicit branch, the directory name becomes a new branch name; if already taken, fufu generates a name. Supply a second argument to choose an existing available branch.

<!-- transcript:independent -->
```console
$ ff worktree ../review
made review at /tmp/opencode/fufu-guide-ZPWYHN/independent/review on review
  on a new branch
  its log is refs/fufu/wt/review/ops

$ printf 'review draft\n' > ../review/review.txt

$ ff -C ../review status
on review · nothing to pull
@  lvuuumsy 1dfd861a   0s ago
│  (no description)
│  A review.txt +1  -0  ++++++++++++++++++++
│    1 file     +1  -0
●  kqnotrwk 6d2f84d5   0s ago
│  demo: initial files

$ ff -C ../review commit -m "review: notes"
closed 6abaafef on review: review: notes (1 file(s))
undo: ff undo

$ printf 'independent feature\n' > app.txt

$ ff commit -m "app: independent feature"
closed fc5b3dff on feature: app: independent feature (1 file(s))
undo: ff undo

$ ff -C ../review undo
undid: commit on review: review: notes
  now at 950934b51dc1 (pre: ff -C ../review status)
  refs/heads/review → 6d2f84d5
back: ff redo

$ ff -C ../review history
↑1  96b8b979d63a    0s ago  redo  commit on review: review: notes
@   950934b51dc1    0s ago  now   pre: ff -C ../review status
↓1  a7b8aab01bb9    0s ago  undo  operation log initialized from observed state; earlier operations not undoable
    (the floor)

```
<!-- /transcript -->

[`ff status`](../reference/cli/status.md) and [`ff commit`](../reference/cli/commit.md) operate in the checkout selected by `-C`. The commit on feature remains when [`ff undo`](../reference/cli/undo.md) reopens the review commit. Inspect both checkouts and continue each task there.

## One repository, a log per tree

[`ff history`](../reference/cli/history.md) in the transcript shows review's own chain. Undo walks the chain of the worktree in which it runs, not the latest operation anywhere in the repository. Creating a worktree records its earliest recovery point; one created by raw Git gets that point on its first fufu command. Uncaptured state before that point is unavailable to undo. Shared refs can still be observed as outside changes by another chain, and branches checked out elsewhere have movement guards.

## Parking crosses trees with its branch

Prerequisite: feature has unfinished work and another checkout can take it after this one switches away. The parked change belongs to the branch, so it can resume in another worktree.

<!-- transcript:resume -->
```console
$ ff worktree ../review
made review at /tmp/opencode/fufu-guide-ZPWYHN/resume/review on review
  on a new branch
  its log is refs/fufu/wt/review/ops

$ printf 'unfinished idea\n' > idea.txt

$ ff switch main
parked the open change on feature (8519b9d2)
switched to main
undo: ff undo

$ ff -C ../review switch feature
ff: absorbed 1 change made outside fufu: refs/heads/feature created at 6d2f84d5, forked from review (branch: forked from main)
switched to feature
resumed the parked change (1 file(s))
undo: ff undo

$ ff switch feature
ff: 'feature' is already used by worktree at '/tmp/opencode/fufu-guide-ZPWYHN/resume/review'
  try:
    ff worktree
    ff -C <path> status

$ ff -C ../review switch review
parked the open change on feature (8519b9d2)
switched to review
undo: ff undo

$ ff switch feature
switched to feature
resumed the parked change (1 file(s))
undo: ff undo

```
<!-- /transcript -->

The first checkout cannot take feature while review holds it. Switching review away parks the edits again; switching this checkout back resumes them here. Inspect the files where the branch is now open. An absorbed-change notice can appear because each chain independently observes shared refs. [Using fufu alongside Git](../concepts/two-regimes.md#returning-after-outside-changes) explains that reconciliation.

## Removal captures first

Prerequisite: the second checkout is no longer needed. Removal captures its eligible working-copy content and prints the operation ID before deleting the checkout; dirty work does not require `--force`.

**Capture limits apply at removal:** ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` are not preserved by the removal capture. Oversized tracked files can retain an older version. Removed chains also expire under `fufu.keep` (90 days by default). Save anything outside that coverage separately, and restore or commit needed captured work before retention removes it.

<!-- transcript:removal -->
```console
$ ff worktree ../review
made review at /tmp/opencode/fufu-guide-ZPWYHN/removal/review on review
  on a new branch
  its log is refs/fufu/wt/review/ops

$ printf 'unfinished review\n' > ../review/review.txt

$ ff worktree -d ../review
removed review (was on review)
  captured first as aa2fd228be1b — ff restore <path> --at-op aa2fd228be1b
  its log stays at refs/fufu/wt/review/ops

$ ff worktree
* main      /tmp/opencode/fufu-guide-ZPWYHN/removal/demo  feature

chains whose worktree is gone
  review    review  aa2fd228be1b  0s ago
ff restore <path> --at-op <op>  brings a file back from one

$ ff restore review.txt --at-op aa2fd228be1b
restored from aa2f (pre: ff worktree -d ../review)
  restored  review.txt
undo: ff undo

```
<!-- /transcript -->

[`ff restore`](../reference/cli/restore.md) uses the exact capture ID printed by removal and repeated in the gone-chain list. The file joins this surviving checkout's open work; its branch, HEAD, and index do not move. Review it with [`ff diff`](../reference/cli/diff.md), then commit when ready. [Retention and recovery](recovery.md#retention-and-the-earliest-recovery-point) explains expiry.

### Bring the whole checkout back

Prerequisite: removal is the latest operation on the chain of the worktree that ran it. This independent recipe undoes immediately, before a later restore or other operation adds work to that chain.

<!-- transcript:removal-undo -->
```console
$ ff worktree ../review
made review at /tmp/opencode/fufu-guide-ZPWYHN/removal-undo/review on review
  on a new branch
  its log is refs/fufu/wt/review/ops

$ printf 'unfinished review\n' > ../review/review.txt

$ ff worktree -d ../review
removed review (was on review)
  captured first as 049ae9eb2807 — ff restore <path> --at-op 049ae9eb2807
  its log stays at refs/fufu/wt/review/ops

$ ff undo
undid: remove worktree review
  now at 85b19a9f2fa6 (add worktree review on review)
back: ff redo

```
<!-- /transcript -->

The checkout and captured uncommitted file return. Inspect it with `ff -C ../review status`, then resume work there. This has the same ignored-file, size, and retention limits as removal's capture; it cannot recreate uncaptured bytes.

## Two writers, one repository

Each worktree's chain has its own write lock, so separate worktrees do not contend for the same operation-log lock. Git's object and ref transactions still guard shared storage, and worktree ownership can prevent moving a branch held elsewhere. A separate chain is not a promise that every concurrent operation can proceed.

### Two processes in one tree

A capture skips when its chain lock is busy. A verb waits up to two seconds, then refuses with `ref/contended` (exit 4); retry it. A skipped capture did not preserve that instant's bytes, so do not assume another process captured your exact file state. [Architecture](../internals/architecture.md#where-fufus-state-lives) locates the locks; [errors](../reference/errors.md) lists refusals.

## Watching every tree

Prerequisite: two live worktrees exist and a script needs to observe their operation logs. [`ff watch`](../reference/cli/watch.md) emits one JSON object per event. This bounded example prints the two initial events and exits, requiring no second terminal or background watcher.

<!-- transcript:watch -->
```console
$ ff worktree ../review
made review at /tmp/opencode/fufu-guide-ZPWYHN/watch/review on review
  on a new branch
  its log is refs/fufu/wt/review/ops

$ ff watch --all -n 2
{"ff":1,"cmd":"watch","data":{"worktree":"main","motion":"start","tip":"61153a83d4ea249a86ea9062d679f6f898cf4186"}}
{"ff":1,"cmd":"watch","data":{"worktree":"review","motion":"start","tip":"a7b8aab01bb9510b21f0c16c25d9f8c097850106"}}

```
<!-- /transcript -->

Bare watch follows the current worktree. `--all` follows all chains, including retained chains after removal; each event names its worktree. Without the count bound, later operations produce events such as `landed`, containing the operation's kind, ID, and session. `--kind` and `--session` filter events. Watch observes log movement; it is not a filesystem watcher and does not create captures for file edits. Use the [watch reference](../reference/cli/watch.md) and [JSON output and scripting](../agents/machine-surface.md) for stream handling.

## From here

- [Changes](../concepts/changes.md) — open work, partial commits, parking, and resuming.
- [Stacked changes](stacked-changes.md) — dependent branches and cascade skips for branches checked out elsewhere.
- [Recovery](recovery.md) — restore files or local branch state from retained operations.
