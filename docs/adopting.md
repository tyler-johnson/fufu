# Adopting fufu

Run [`ff init`](reference/cli/init.md) inside an existing Git repository to enable snapshots and local operation history. No import or conversion is needed. Your existing commits and branches stay in place, and teammates do not need to install fufu.

```sh
ff init
```

<a id="what-arming-does"></a>

## What initialization does

Initialization writes local Git configuration to protect fufu's recovery refs from ordinary garbage collection, records the earliest recovery point from the observed repository state, and takes an initial working-copy snapshot. Later repository commands take further snapshots. fufu does not continuously watch the filesystem.

[`ff undo`](reference/cli/undo.md) can reach retained states recorded from this point onward; enabling fufu does not make earlier uncommitted work recoverable retroactively. [Snapshot coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits) describe excluded files, size limits, retention, and worktree scope.

## Install shell and agent hooks

Initialization is local to the repository. [`ff hook`](reference/cli/hook.md) installs shell and agent integrations separately. Follow the [hook setup](reference/hooks/index.md) instructions to activate them in the sessions you use; installed files alone do not make every editor or script invoke fufu.

[`ff doctor`](reference/cli/doctor.md) reports repository setup and installed integrations. It also attempts snapshot/reconciliation, can fetch when enabled, and can run maintenance; it is not a read-only inspection command.

<a id="what-does-not-change"></a>

## Using your existing tools

Git, IDEs, GUIs, remotes, and CI keep using the same repository. Initialization adds fufu's local config and refs without rewriting branch history or installing shell and agent hooks. Git views that include all refs can show fufu's saved objects too.

[Using fufu alongside Git](concepts/two-regimes.md) explains which commands park work, what shell aliases and policy checks cover, and how fufu reports changes made by other tools.

## The workflow shift

Start with [Working copy and commits](concepts/changes.md): edit files, commit without staging, and switch tasks with unfinished work parked on its branch. Updates replay branch commits onto their bases rather than merging the base into each task branch.

Before sending rewritten work, read [Pulling and pushing](concepts/push-boundary.md) for the lease rules and the distinction between local rewriting and a team's shared-history policy.

## Adopting mid-flight

- **Uncommitted work:** a dirty working copy is accepted. The initial snapshot records it subject to coverage and successful recording; check [`ff history`](reference/cli/history.md) to see the available recovery steps.
- **A Git operation in progress:** finish or abort an existing merge, rebase, or bisect with Git. fufu does not take over that session.
- **Existing stashes:** your Git stash entries are left alone. A parked change from an older fufu version that used a recorded stash entry is converted to an open-change commit when you first switch to that branch; personal stashes are not applied or removed.

## Trying it and leaving

You can use Git between fufu commands or remove the binary later. [Leaving and coming back](concepts/two-regimes.md#leaving-and-coming-back) is the main reference for accessing parked work with Git and what fufu can observe when you return.
