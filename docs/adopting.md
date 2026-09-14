# Adopting fufu

Run [`ff init`](reference/cli/init.md) inside an existing Git repository to enable snapshots and local operation history. No import or conversion is needed. Your existing commits and branches stay in place, and teammates do not need to install fufu.

```sh
ff init
```

## The workflow shift

- Edit files, then [`ff commit -m "message"`](reference/cli/commit.md) records eligible changes without staging.
- [`ff switch`](reference/cli/switch.md) saves unfinished work with the branch you leave and resumes the destination branch's work. Returning brings the saved edits back.
- [`ff pull`](reference/cli/pull.md) updates the current branch from its base and remote copy by replaying commits. [`ff push`](reference/cli/push.md) sends branch updates separately.

Try the [tutorial](tutorial.md) in a fresh clone of fufu's own repository, or read [Working copy and commits](concepts/changes.md) for the lifecycle. Before sending rewritten work, read [Pulling and pushing](concepts/push-boundary.md) for lease rules and shared-history policy.

<a id="what-arming-does"></a>

## What initialization does

Initialization writes local Git configuration to protect fufu's recovery refs from ordinary garbage collection, records the earliest recovery point from the observed repository state, and takes an initial working-copy snapshot. Later repository commands take further snapshots. fufu does not continuously watch the filesystem.

[`ff undo`](reference/cli/undo.md) can reach retained states recorded from this point onward; enabling fufu does not make earlier uncommitted work recoverable retroactively. [Snapshot coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits) describe excluded files, size limits, retention, and worktree scope.

## Install shell and agent hooks

Initialization is local to the repository. [`ff hook`](reference/cli/hook.md) installs shell and agent integrations once per machine, separately. If you have not installed and activated them, follow [installation](install.md#install-hooks); installed files alone do not make every editor or script invoke fufu.

[`ff doctor`](reference/cli/doctor.md) reports repository setup and installed integrations. It also attempts snapshot/reconciliation, can fetch when enabled, and can run maintenance; it is not a read-only inspection command.

<a id="what-does-not-change"></a>

## Using your existing tools

Git, IDEs, GUIs, remotes, and CI keep using the same repository. Initialization adds fufu's local config and refs without rewriting branch history or installing shell and agent hooks. Git views that include all refs can show fufu's saved objects too.

[Using fufu alongside Git](concepts/two-regimes.md) explains which commands park work, what shell aliases and policy checks cover, and how fufu reports changes made by other tools.

## Adopting mid-flight

- **Uncommitted work:** a dirty working copy is accepted. The initial snapshot records it subject to coverage and successful recording; check [`ff history`](reference/cli/history.md) to see the available recovery steps.
- **A Git operation in progress:** finish or abort an existing merge, rebase, or bisect with Git. fufu does not take over that session.
- **Existing stashes:** your personal Git stash entries are left alone. Apply them with Git when you want their contents in the working copy.

## Trying it and leaving

You can use Git between fufu commands or remove the binary later. First resume and commit any parked work you want in ordinary branch history. [`ff unhook`](reference/cli/unhook.md) removes the integrations you select; restart the affected shells and clients afterward. Your commits and branches remain usable with Git. [Leaving and coming back](concepts/two-regimes.md#leaving-and-coming-back) covers accessing parked work directly and what fufu can observe when you return.
