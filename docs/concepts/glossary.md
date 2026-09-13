# Glossary

## A–C

**automatically named branch** — An ordinary branch given a generated name, such as `ff/hidden-wren`, when you create it without choosing one. See [branch naming](branches.md#automatically-named-branches).

**base branch** — The branch your work builds on, such as `main` for `parser-fix`. See [base branch and remote copy](branches.md#base-branch-and-remote-copy).

**capture** — The technical name in command output for a working-copy snapshot. See [when snapshots run](snapshots-and-undo.md#when-snapshots-run).

**cascade** — The replay of child branches after a command rewrites their base branch. See [cascades](branches.md#the-cascade) for holds and skipped branches.

**chain** — One worktree's recorded operation history. See [worktree-specific recovery](snapshots-and-undo.md#coverage-and-limits).

**change** — A unit of work that can be open in the working copy, parked with a branch, or recorded as a commit. See [working copy and commits](changes.md).

**change ID** — An identifier using k–z that follows a surviving change through fufu rewrites, even when its commit hash changes. See [change identity](changes.md#a-change-has-an-identity).

**close** — Record the open change, or selected paths from it, in branch history with [`ff commit`](../reference/cli/commit.md). See [committing work](changes.md#closing-is-the-commit).

**commit hash** — The hexadecimal ID of a Git commit object; changing the object's content, message, or parents changes the hash. An [internal open-change object](changes.md#internal-storage-and-branch-history) can have a hash before work is committed to branch history.

**current branch** — The branch selected in this worktree, whose files you are working on. See [creating and switching](branches.md#creating-and-switching).

<a id="fl"></a>

## D–L

**earliest recovery point** — The oldest recorded state still available to restore. Initializing fufu records the first observation, called a *floor* in output; retention can shorten the available history. See [recovery's starting point](snapshots-and-undo.md#earliest-recovery-point).

**foreign operation** — A record of ref changes fufu observes after Git or another tool changed the repository. It does not reconstruct intermediate working copies; see [returning after outside changes](two-regimes.md#lazy-absorption).

**held rewrite** — A requested rewrite waiting on a conflict, with that branch's conflicting replay unapplied. See [resolving a hold](held-rewrites.md#resolve-edit-finish).

**lease** — A push check requiring the remote ref to match its expected value when the server updates it. See [push leases](push-boundary.md#push-carries-a-lease) for the separate seen-record and team-policy rules.

## M–P

**map** — The view of branches and recent work drawn by bare `ff`, also available as [`ff map`](../reference/cli/map.md).

**open change** — The files you are editing now, including any pending commit message; it can be empty. See [working copy and commits](changes.md).

**operation** — A recorded snapshot, command action, or observed outside ref change. See [the operation log](snapshots-and-undo.md#one-log-one-address-space).

**operation ID** — A hexadecimal identifier for one operation, used in operation arguments and `--at-op`. See [addressing an operation](snapshots-and-undo.md#addressing-an-operation).

**operation log** — The worktree's record of snapshots and local actions, listed by [`ff op log`](../reference/cli/op-log.md). See [snapshots and undo](snapshots-and-undo.md#one-log-one-address-space).

**parked change** — Uncommitted work saved with a branch when you switch away, then resumed when you return. See [parking and resuming](changes.md#parking-and-resuming).

**pending description** — The open change's planned commit message, set with [`ff describe`](../reference/cli/describe.md). See [pending descriptions](changes.md#pending-descriptions).

**pull** — Update local branches from their bases and remote copies with [`ff pull`](../reference/cli/pull.md). See [pulling local updates](push-boundary.md#pulling-local-updates).

**push** — Send selected branches to their remote copies with [`ff push`](../reference/cli/push.md). See [choosing what to push](push-boundary.md#choosing-what-to-push).

<a id="rt"></a>

## R–W

**remote copy** — The published branch corresponding to a local branch, such as `parser-fix` on `origin`. See [branch relationships](branches.md#base-branch-and-remote-copy).

**remote-tracking ref** — A local record such as `origin/parser-fix` of a remote branch's last fetched position. See [tracking](branches.md#tracking-one-branch-one-remote-copy).

**replay** — Reapply commits' changes on an updated base, creating new commit objects. See [stacking](branches.md#stacking-a-branch-records-its-parent) and [conflicts](held-rewrites.md).

**restack** — Replay a branch's commits onto its base's current tip with [`ff restack`](../reference/cli/restack.md). See [stacking](branches.md#stacking-a-branch-records-its-parent).

**run** — Adjacent snapshots from the same session, grouped into one undo step. See [undo grouping](snapshots-and-undo.md#undo-steps-over-runs).

**snapshot** — Saved file state for recovery, taken automatically or manually without adding a commit to branch history. See [snapshot coverage and limits](snapshots-and-undo.md#coverage-and-limits).

**trunk** — The repository's main development branch and the default base for new work. See [base selection](branches.md#base-branch-and-remote-copy).

**undo step** — One move backward in [`ff history`](../reference/cli/history.md): a recorded command operation or a grouped run of snapshots. See [reading history](snapshots-and-undo.md#reading-ff-history).

**worktree** — A checkout with its own files, index, HEAD, and operation chain, sharing the repository's objects and branches. See [worktrees](../guides/worktrees.md).
