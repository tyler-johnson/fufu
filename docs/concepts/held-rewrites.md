# Conflicts and held rewrites

<a id="held-rewrites"></a>

When a replay or merge conflicts, fufu reports a **held rewrite**: the requested rewrite is waiting for you to resolve it. [`ff status`](../reference/cli/status.md) shows `held:`, the command that caused it, the conflicting commit and files, and what to do next.

By default, the conflicting operation has not advanced that branch's tip or put markers in your working copy. Earlier successful branch updates in the same command can stand. You can keep working at the existing tip, or resolve the conflict now.

<a id="ff-resolve-all-of-it-at-once"></a>

## Resolve, edit, finish

On the held branch, [`ff resolve`](../reference/cli/resolve.md) opens a resolution session and puts the surviving conflicts into files as labeled markers. Edit those files, remove the markers, then run [`ff done`](../reference/cli/done.md) to apply the fixes and return to the original branch.

```sh
ff resolve
# Edit the conflicting files and remove the conflict markers.
ff done
```

You do not need to stage the fixes. If another branch is held, first use [`ff switch`](../reference/cli/switch.md) to select it. A parked change that conflicts when you switch back is a [distinct case](#parked-change-arrival), resolved in place.

To open the session immediately, add `--resolve` to [`ff restack`](../reference/cli/restack.md), [`ff pull`](../reference/cli/pull.md), or [`ff merge`](../reference/cli/merge.md). The command records the hold and opens the current branch's session in one run. Use `ff config onConflict resolve` to make this the default, or `--no-resolve` to stop at the hold for one run.

### The session

A rewrite resolution session uses an automatically named branch whose starting commit contains the marker tree. Your original branch keeps its tip and hold; its open change parks there. Status shows `resolving:` while the session is open.

The current side of a marker is labeled `the rewrite so far`. The incoming side identifies the replayed commit, for example `>>>>>>> rebasing "add parser options" (3/10)`. These labels associate each fix with the step that needs it.

You can switch away from the session and return later. Its unfinished edits park and resume like other branch work. The temporary session commit contains literal markers that ordinary Git can read; the original branch does not contain that marker commit.

### Landing the session

`ff done` applies each fix to its corresponding replay step and reruns the rewrite. When it succeeds, the rewritten branch receives the clean commits, the session branch is deleted, and you return to the original branch with its parked work restored or reported as a held arrival. Branches based on the rewritten branch can then follow through a [cascade](branches.md#the-cascade).

Some conflicts take more than one round. Fix the displayed conflicts and run `ff done`. If more conflicts appear, repeat on the same session. Each new round keeps applicable fixes, names any that need revisiting, and exits 3; the original branch updates when all conflicts are resolved. One [`ff undo`](../reference/cli/undo.md) reverses the advance to the next round and restores the fixes you had made before it.

Another round is needed when fixing one conflict exposes another in a later replayed commit. Fufu stops before writing overlapping marker blocks. A conflict on either parent of a merge is shown first; after it is fixed, fufu applies the merge's own changes to that file, which can produce a further conflict.

## Abandoning or undoing a resolution

`ff resolve --abandon` drops the held rewrite and an open resolution session, returning from the session if needed. It works from the session or the held branch. [`ff done --abandon`](../reference/cli/done.md) from the session closes only the session: the hold stays, and `ff resolve` opens a fresh session over it.

Opening a rewrite session takes two operations: creating the session branch and switching to it. One `ff undo` returns to the original branch; another removes the newly created session. Landing, closing, or abandoning is one operation, so one undo restores the session and its recorded fixes either way, and the hold with them when it was dropped. [Snapshot coverage and retention](snapshots-and-undo.md#coverage-and-limits) apply.

## Parked-change arrival

A **held arrival** happens when `ff switch` cannot replay parked work over a branch tip that has moved. The branch switch still completes and reports exit 3, but the parked edits wait for resolution.

Here `ff resolve` lays the parked change into the current working copy with markers. It does not create a resolution-session branch: the result is an open change. Edit the markers, then continue working or use [`ff commit`](../reference/cli/commit.md) when ready to record it. There is no session to finish with `ff done`.

If the branch already has another open change, resolve refuses to overwrite it. Commit that work before retrying; switching away only parks it on the same branch, so returning resumes the blocker. `ff resolve --abandon` drops a still-held arrival and reports the saved parked commit; it does not apply that work.

<a id="deferred-requires-loud"></a>

## Reading conflict reports

The command announces the hold when it is created. Status keeps showing it until it is resolved or abandoned, and [`ff branch`](../reference/cli/branch.md) marks held branches and unfinished sessions in its list.

Exit 3 reports a held primary replay for [`ff pull`](../reference/cli/pull.md), [`ff restack`](../reference/cli/restack.md), `ff done`, [`ff absorb`](../reference/cli/absorb.md), and [`ff lift`](../reference/cli/lift.md), and a held auto-merge for [`ff merge`](../reference/cli/merge.md). Pull, restack, and [`ff fold`](../reference/cli/fold.md) also exit 3 for holds in their cascades. A successful absorb, lift, or session landing can return 0 with a downstream branch held, because its primary change landed. A reword through [`ff describe`](../reference/cli/describe.md) currently returns 0 even when its cascade holds; scripts must inspect `reword.cascade.held`. Read the branch reports as well as the exit code.

## What a hold blocks, and what it does not

[`ff push`](../reference/cli/push.md) refuses to send a held branch. Resolve or abandon its pending rewrite before pushing. This is a guard on fufu's push command; [Git compatibility and policy](two-regimes.md#which-program-ran) explain what happens with other callers.

A hold does not itself prevent local commits or branch switches. You can keep building on the existing tip, but the requested post-rewrite state is not available until it applies successfully. Other command-specific guards still apply.

## What a hold records

A hold saves the requested rewrite's intent: its branch and target. Resolution recomputes the replay against current inputs instead of resuming an old partial plan. Work committed at the existing tip can therefore be included when you resolve later.

A held restack requests a replay onto a target branch. A held merge requests a merge commit: it can come from `ff merge <branch>`, or from `ff resolve` taking in updates to a previously merged base. Absorb, lift, done, and parked-change arrivals can also record holds, carrying work still waiting to be applied.

If a held replay now applies cleanly, `ff resolve` releases the hold and tells you to rerun the original command. A held merge that is now clean lands directly; if its target is already included, resolve releases the hold. A target that disappeared or no longer belongs to the required history can make the hold expire instead.

## What a rewrite does to a standing hold

A held restack or merge records a target rather than extra file content. A later rewrite can therefore update that request:

- `ff restack --onto` another base drops the hold and reports it. Folding the held source branch also drops its hold.
- A replay onto the hold's target clears it: the requested restack has landed, or replay has replaced the need for that merge.
- Other rewrites keep the hold and update its recorded commit. This includes absorb, lift, describe, and an editing-session landing with done.

Use `ff status` to inspect the remaining hold.

A held absorb, lift, done, or parked-change arrival carries work that is not on the branch yet, so every rewrite of the branch refuses with `held/already-held`. Its exits are `ff resolve` and `ff resolve --abandon`. A hold whose resolution session is open, whether you are standing in it or have parked it, refuses with `held/resolving` and names the session.

The rewrite and its change to the hold form one operation. One `ff undo` reverses both.

## How conflicts reach the session

fufu replays commits in memory before updating the branch. During resolution it carries unresolved regions forward as literal marker content, letting later commits apply around them. A conflict already fixed by a later commit can disappear before the session opens. The remaining regions are presented together, except where overlapping conflicts take another round on the same session.

<a id="why-not-a-conflicted-commit"></a>

For the design choice between a pending rewrite and jj's conflict objects, see [fufu vs jj](../comparisons/vs-jj.md). The [storage model](invariant.md) explains how session and parked-change objects remain readable by Git.
