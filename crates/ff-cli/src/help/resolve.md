Open the current branch's held rewrite for conflict resolution. Resolve puts the conflicts into the working copy as labeled markers. Edit them, then run `ff done`; repeat if it shows another round of conflicts. With no hold, resolve can also take base updates into a branch whose `fufu.pull` policy is `merge`, or `auto` with a merge among its commits.

## Examples

```sh
ff resolve                      # Open the recorded conflicts
# Edit the marked files, then finish the rewrite session:
ff done
ff resolve --abandon            # Alternatively, drop the pending rewrite
```

### Options

### Rewrite sessions

A resolution session is an automatically named branch containing the conflict markers. The hold remains on the original branch, where your previous open work is parked. Switching away parks fixes in progress; switching back resumes them. Done applies the fixes and returns in one operation when all conflicts are resolved. If the fixes uncover more conflicts, it keeps the session open for another round, with exit 3.

If a held replay now applies cleanly, resolve releases the hold instead. Re-run the original command to apply that replay. A held merge is different: resolve lands it directly when it is clean, or releases the hold if the target is already included.

### Taking the base in

An existing hold takes priority. Without a hold, resolve merges base updates only when this branch is behind its base and its [`fufu.pull`](../config.md#common-settings) policy resolves to `merge`, or to `auto` with any merge among the commits since their fork. Switch to the branch first; only the current branch is updated. A branch resolving to `replay`, or to `auto` on a straight line, is `ff restack`'s, and the refusal names the policy and where it was set. An up-to-date branch, one with no base or shared history, or an editing session is refused with an explanation.

The new commit has the branch's tip first and the base's tip second, your author identity, a change ID, and a signature when `commit.gpgsign` is enabled. Its subject is `merge <base> into <branch>`. Existing commits are preserved and open work is reapplied over the merge. One `ff undo` removes it. No commit hook runs for this direct merge.

If the merge conflicts, resolve records a hold and opens the resolution session in the same run, with exit 3. Edit the markers and run `ff done`. You can also open sessions immediately with `ff restack --resolve`, `ff pull --resolve`, or `ff merge --resolve`.

A later rewrite can drop, clear, or update a held merge, as it can a held restack. See [standing holds](../../concepts/held-rewrites.md#what-a-rewrite-does-to-a-standing-hold).

### Parked-change arrivals

A held arrival occurs when `ff switch` cannot replay parked work onto a moved branch tip. Resolve handles it in place: the marked files become the open change, with no session and no done step. Fix them and continue ordinary work. Resolution refuses if the branch already has an open change; commit that work first. Switching away only parks it on the same branch, so returning resumes the blocker.

### Abandoning and undo

`--abandon` drops the hold and any open resolution session, returning to the original branch. `ff done --abandon` from the session is the way to close it without dropping the hold. Opening a rewrite-resolution session is two operations, creation and switching; two `ff undo` calls take it back. Landing or abandoning is one operation. `ff history` shows the available steps.

Opening a session over an existing hold exits 0. Recording a new conflicting base merge and opening its session exits 3. Advancing a session to another conflict round with `ff done` also exits 3 and takes one undoable operation. `held/none` exits 3, like other `held/` refusals.
