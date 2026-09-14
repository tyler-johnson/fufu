# Plain-git teammates

Your teammates can keep Git, their GUI, and their existing review and CI workflow. Fufu writes ordinary Git branch history. You need fufu locally for its commands and active hooks for additional capture boundaries; teammates do not need to install either.

| Where someone looks | What they see |
| --- | --- |
| Another clone after a normal branch push | Ordinary commits, branches, and files. Fufu's local operation and parked-work refs are not sent. |
| This checkout with `git status`, `git diff`, or `git log` | The real working copy, index, and current branch history. |
| This repository with `git log --all` or a ref browser | Internal refs too: parked/open commits and operation records under `refs/fufu/`. |

The [compatibility concept](../concepts/two-regimes.md) is the main explanation of execution paths and reconciliation. These recipes cover practical tasks. Each starts independently in an initialized scratch repository on `feature`, based on main, with `app.txt` containing `hello` and a `README.md`. `bash scripts/docs/plain-git-teammates-transcript.sh` runs all recipes; pass an ID such as `passthrough` for one. The teammate recipe also creates a local origin and another clone.

## What everyone else sees

Prerequisite: a branch is ready to publish. [`ff commit`](../reference/cli/commit.md) records the work, then [`ff push`](../reference/cli/push.md) sends its history. The remaining commands inspect it from a plain-Git clone.

<!-- transcript:teammate -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed 2029c40a on feature: app: feature (1 file(s))
undo: ff undo

$ ff push
created origin/feature and set feature to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

$ git -C ../teammate fetch -q origin

$ git -C ../teammate switch -q feature

$ git -C ../teammate log --oneline -2
2029c40 app: feature
c0bc30c demo: initial files

$ git -C ../teammate status --short

```
<!-- /transcript -->

The teammate gets the same branch tip and a clean checkout. CI can build it normally. History commits can carry a `change-id` header visible through Git plumbing; no special reader is required. A normal fufu branch push does not send `refs/fufu/`. Explicit raw Git mirror or refspec pushes are a different operation. Continue with the team's normal review process.

## Recover parked work with Git

Prerequisite: work was parked by [`ff switch`](../reference/cli/switch.md) and its open ref is retained. [`ff describe`](../reference/cli/describe.md) supplies the pending description here. Inspect the exact parked ref rather than guessing from the first row of `git log --all`.

<!-- transcript:parked -->
```console
$ printf 'tuning pass\n' > app.txt

$ ff describe -m "app: tuning pass"
pending description on feature: app: tuning pass

$ ff switch main
parked the open change on feature (bf9945cb)
switched to main
undo: ff undo

$ git log refs/fufu/open/feature -1 --oneline
bf9945c app: tuning pass

$ git log refs/fufu/wt/main/ops -1 --oneline
8ed4d1c switch from feature to main

$ git cherry-pick -n refs/fufu/open/feature

```
<!-- /transcript -->

The first Git log command shows the parked change; the second shows an operation record. Both are ordinary commit objects, but neither is a commit recorded on main. Cherry-pick with `-n` applies the parked patch to the current checkout and index without committing. Inspect `git diff --cached`, then continue with Git if fufu is unavailable. With fufu available, `ff switch feature` normally resumes the parked change directly. Applying a patch this way can conflict if the receiving branch differs.

## Your own git tools: reads and writes

Prerequisite: an IDE or raw Git recorded a wanted commit outside fufu. Ordinary reads continue to work; a raw write is reconciled on the next fufu command.

<!-- transcript:outside -->
```console
$ printf 'IDE edit\n' > app.txt

$ git commit -am "app: IDE edit"
[feature b3663c3] app: IDE edit
 1 file changed, 1 insertion(+), 1 deletion(-)

$ ff status
on feature · nothing to pull
@  no changes
│  (no description)
●  uvrvoyyu b3663c31   0s ago
│  app: IDE edit
1 change made outside fufu: refs/heads/feature moved to b3663c31 (absorbed; ff undo can roll it back)

```
<!-- /transcript -->

[`ff status`](../reference/cli/status.md) reports the observed ref movement as foreign work. The commit remains in place, ready for further editing or a push. Reconciliation records observed state; it does not reconstruct every intermediate file edit made while fufu was absent. Recovery reaches successfully captured, retained states, not every action performed by the other tool.

<a id="ff-git-the-escape-hatch-that-keeps-undo-working"></a>
## Capture before a Git command

Prerequisite: Git has a command you need and the configured policy permits it. [`ff git`](../reference/cli/git.md) attempts capture before running the requested Git command. This disposable example includes uncommitted bytes immediately before a destructive reset.

<!-- transcript:passthrough -->
```console
$ printf 'feature\n' > app.txt

$ ff commit -m "app: feature"
closed d9db9f9b on feature: app: feature (1 file(s))
undo: ff undo

$ printf 'uncommitted draft\n' > app.txt

$ ff git reset --hard HEAD~1
ff: tip: that's ff undo
HEAD is now at 6d2f84d demo: initial files

$ ff undo
ff: absorbed 1 change made outside fufu: refs/heads/feature moved to 6d2f84d5 (reset: moving to HEAD~1)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at b1cc30470b38 (pre: git reset --hard HEAD~1)
  refs/heads/feature → d9db9f9b
  1 worktree file(s) restored
back: ff redo

```
<!-- /transcript -->

[`ff undo`](../reference/cli/undo.md) restores the branch tip and the captured uncommitted draft. Inspect the recovered files before continuing. Capture excludes ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` (50 MiB by default); oversized tracked files may retain older content. Capture success and retention still matter. See [recovery](recovery.md) for choosing a point after later work.

## The alias and gitPolicy

Prerequisite: you want typed Git commands to go through fufu, or want guidance when they overlap its commands. [`ff hook bash`](../reference/cli/hook.md), or the corresponding zsh, fish, or PowerShell hook, installs shell integration including a Git alias. Activate it in that shell as the [client reference](../reference/hooks/bash.md) directs. Scripts, GUIs, and executables resolving Git directly on PATH do not inherit a shell alias.

[`ff config gitPolicy`](../reference/cli/config.md) selects the policy on the fufu execution path:

- `observe` runs without coaching.
- `coach`, the default, suggests the fufu equivalent once per Git word and runs Git.
- `strict` refuses covered Git writes with a fufu equivalent. It does not substitute another command.

<!-- transcript:policy -->
```console
$ ff config gitPolicy strict
gitPolicy = strict (this repo)

$ ff git commit -m wip
ff: fufu.gitPolicy is strict and refused git commit; review ff commit and the policy's scope before retrying
  try:
    ff explain usage/git-policy
    ff config gitPolicy coach

$ ff git log --oneline -1
6d2f84d demo: initial files

```
<!-- /transcript -->

The refused commit does not run; strict passthrough refusal precedes its capture, though the policy tally is recorded. Reads still pass. Active agent hooks have their own received-event coverage and attempt capture before policy evaluation; [agent setup](../agents/setup.md) explains activation. Verify the intended shell or client integration rather than assuming installing a hook enabled every execution path.

## A weekend away

You can keep using Git on a machine without fufu. On returning, the next fufu command reconciles observed branch and ref changes. Files that were created and lost between captures cannot be recovered from a later observation. Inspect status, then use the ordinary fufu workflow again. [Leaving and coming back](../concepts/two-regimes.md#leaving-and-coming-back) has the details.

## What fufu asks of the branch, and of the repo

No server hook or team-wide conversion is required. Merge, squash, and rebase policy remains the team's choice. [`ff branch --prune`](../reference/cli/branch.md) can remove local branches whose previously observed remote copies are gone; review its selection before cleanup.

Fufu can rewrite pushed commits locally and send them with a separate leased push. It does not enforce branch ownership or protect main specially. Use server-side protection for shared-history policy. If a teammate force-pushes over your branch, [recover and reconcile](recovery.md#someone-force-pushed-over-my-branch) before retrying. [Pulling and pushing](../concepts/push-boundary.md) explains what the lease checks.
