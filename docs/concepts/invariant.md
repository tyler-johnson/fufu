# Git storage model

fufu stores its work in Git objects and refs. Branches remain ordinary Git branches, and the history sent to a remote consists of ordinary commits. For day-to-day behavior with IDEs, raw Git, or a machine without fufu, read [Using fufu alongside Git](two-regimes.md). New readers can start with [Working copy and commits](changes.md).

## The invariant

The design rule is to use states Git can represent and read. fufu creates branches when selecting historical revisions, stores parked work as commits, and records a conflicting rewrite as pending intent rather than updating the original branch to an unresolved result.

This does not hide every internal object from Git tooling. A view including all refs can display snapshots, open-change commits, and temporary resolution-session commits containing literal conflict markers.

## Objects outside branch history

[Snapshots](snapshots-and-undo.md) use refs outside the branch history normally read by teammates. An [open or parked change](changes.md#internal-storage-and-branch-history) is a commit under `refs/fufu/open/<branch>`. Updating that object saves work without advancing the branch.

A [resolution session](held-rewrites.md#the-session) uses a temporary branch at a marker-containing commit. Finishing the session applies fixes to the replayed commits before updating the original branch. [Architecture](../internals/architecture.md) documents the full ref layout and operation records.

<a id="a-cache-over-git-never-an-authority"></a>

## Git state and fufu records

Git refs and objects describe the repository's current state. fufu's operation log and metadata add information Git alone does not record: earlier captured working copies, branch-associated parked work, rewrite relationships, and pending rewrite intent.

When outside tools change refs, fufu reconciles its records with the observed repository. Those records cannot reconstruct every intermediate state or infer every foreign rewrite. The [compatibility page](two-regimes.md#lazy-absorption) explains the observable behavior and recovery limits.

<a id="deleting-fufu-loses-convenience-never-data"></a>
<a id="reconciliation-is-loud"></a>

For removing the binary, accessing saved work with Git, and returning later, see [Leaving and coming back](two-regimes.md#leaving-and-coming-back).

<a id="compatibility-not-neutrality"></a>

## Workflow choices

The storage format permits a particular workflow: working-copy commits without staging, automatic snapshots, parked work, and replay-based updates. It does not enforce a team's shared-history policy; [pulling and pushing](push-boundary.md#shared-history-policy) describes that boundary. The design comparisons belong in [fufu vs Git](../comparisons/vs-git.md) and [fufu vs jj](../comparisons/vs-jj.md).
