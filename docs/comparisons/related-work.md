# Related work

These tools offer related ways to inspect work, edit history, and recover from mistakes. The links below identify the source for each comparison; unversioned project documentation was reviewed on September 13, 2026.

## jj

jj combines working-copy snapshots, descendant rewrites, an operation log, and first-class conflicts. Its Git-backed colocated mode supports using Git commands alongside jj. [fufu vs jj](vs-jj.md) compares the practical differences against jj v0.45.0, including bookmarks, Git views, and conflict resolution.

## jog

The [founding design](../internals/design.md) credits jog as fufu's proving ground for working-copy capture, shell integration, and recovery. That is project lineage, not a current compatibility or feature comparison. fufu adds its own branch-switching and rewriting workflow; see [snapshots and undo](../concepts/snapshots-and-undo.md) for its current capture contract.

## Sapling

Sapling's [introduction](https://sapling-scm.com/docs/introduction/) describes a source control system developed at Meta with a CLI that can clone Git repositories. Its examples cover smartlog, stacked commits, automatic restacking after amend, optional bookmarks, and recovery with unamend.

These are useful points of comparison for fufu's [stacked-branch workflow](../guides/stacked-changes.md). Sapling's stated scale goals include its server-backed deployment; fufu's [performance evidence](../performance.md) comes from small local fixtures.

## git-branchless

The [git-branchless v0.10.0 README](https://github.com/arxanas/git-branchless/blob/v0.10.0/README.md) describes a suite that extends Git with smartlog, undo, restack, move, and speculative merges. It supports anonymous branching **and ordinary branches**.

Its documented undo limitations include untracked-file edits and staging/unstaging. fufu instead records eligible working-copy content in its operation log, with its own [coverage and retention limits](../concepts/snapshots-and-undo.md#coverage-and-limits). Neither description should be read as unlimited recovery.

## GitButler

GitButler ships [desktop and command-line clients](https://docs.gitbutler.com/). Its [parallel branches](https://docs.gitbutler.com/features/branch-management/virtual-branches) let multiple branches contribute to one working directory, with changes staged and committed independently for each.

fufu switches between one current branch's working changes and another's, or uses separate [Git worktrees](../guides/worktrees.md).

<span id="the-unclaimed-square"></span>

## Where fufu fits

fufu combines working-copy snapshots, current Git branches, per-branch parked changes, and recorded history rewrites. Its [Git comparison](vs-git.md) covers the workflow costs, and its [jj comparison](vs-jj.md) covers the conflict and identity models.
