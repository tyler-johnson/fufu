Lands the branch you are standing on onto another and takes the branch away: its commits replay onto the target's tip, the target fast-forwards to the result, the branch is deleted the way `ff branch delete` deletes one, and this worktree moves to the target with the open change still open. Trunk is the target when you name none. One operation, so one `ff undo` takes all four moves back.

Nothing is merged. The history that lands is linear, the same shape `git rebase --onto` and then `git merge --ff-only` would leave, with the branch's pointer into the log parked under trash and its tip pinned. An anonymous branch — the one a bare `ff start` mints — has no name to lose, and folding it is how a bay lands.

- The replay carries the branch's own commits and no others, bounded where it forked from the target, and the open change replays as the last step. A replay that would conflict refuses with nothing changed; `ff restack --onto <target>` holds the same replay so `ff resolve` can pick it up, and `ff fold` lands it once it is clean.
- A branch that already sits on the target's tip, or ahead of it, replays nothing: the target moves to its tip.
- Trunk cannot be folded, a branch cannot be folded into itself, and a target on a remote is refused, since fold lands into a local branch only. A target another worktree has checked out is refused unless `--stay` says to advance it there.

### Branches stacked above

The branches stacked on the folded branch follow it onto the target's new tip, parent before child, in the one operation, and each records the target as its base, since the branch it sat on is gone. A branch above whose replay conflicts is held where it stands with everything above it left alone, and `ff switch` to it and `ff resolve` picks the replay up. One checked out in another worktree, one already holding a rewrite, and one whose commits hold a merge are skipped and named.

### What it reports

The output says what landed, how far the target moved, what was deleted and where its timeline went, what followed, held, and was skipped, and where you now stand. `--json` carries the cascade as `cascade`. The exit is 3 when a branch above held.

### --stay

`--stay` keeps this worktree on the branch and keeps the branch: it moves to the new tip with the target recorded as its base, sitting on the target with nothing of its own, and the open change stays open here. When another worktree holds the target, the target advances there — its files move to the new tip with its uncommitted work carried over, and refused with nothing written when that work would conflict — and the operation is written on both chains, each half naming the other, so `ff undo` in either tree takes back that tree's half and says what the other still holds. With no other worktree on the target, `--stay` is the same fold minus the deletion and the switch, so a script can pass it unconditionally.

## Examples

```
ff fold                        land this branch on trunk and delete it
ff fold release-1.2            land it on another branch instead
ff fold --stay                 the target is open in another worktree:
                               advance it there, keep this branch here
ff undo                        put the branch back, the target too
```
