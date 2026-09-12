Lines of work. Bare `ff branch` says what exists, `ff branch <name> [<rev>]` creates one without moving there, `ff branch -d <name>` takes one away, and `ff branch --prune` takes away every one whose shared copy is gone. `ff br` is the short spelling, and `ff bookmark` is jj's name for the same verb.

Naming is not here. `ff describe -b <name>` names the branch you are on, on the same axis as -m — one verb for saying what work is, whether the subject is the change's description or the branch's name.

### The list

Named branches first, then the anonymous ones — the petnames fufu mints when work starts without a name — kept apart so a petname never reads as something you chose. Each row carries its tip, the subject there, and what is hanging off it: a parked change, a pending description, and how it stands against its upstream.

Then what a remote is holding that is not here: the branches a clone or a fetch left a tracking ref for and no local branch of yours tracks. Those rows wear the brackets too, because the brackets mean a name you can type at `ff switch`, and these are: `ff switch origin/<branch>`, or bare `ff switch <branch>`, is what makes the local branch, tracking the remote's.

The section is bounded the way the map is, with a dim count standing for the rest; --all is that bound spelled off.

### Creating

`ff branch <name>` mints a branch at trunk's tip and leaves you where you stand. `<rev>` forks it elsewhere: a revision puts the tip on that commit, and a branch name puts it on that branch's tip and records the branch as the parent, so the new one has a base to be measured against. `@` puts the tip on the commit under the open change and parks a copy of the open change on the new branch, so `ff switch <name>` resumes it there; nothing moves and nothing parks here.

`ff start` is the verb that also moves there. Creating is one operation, so `ff undo` takes the whole of it back.

### Deleting

The branch's pointer into the log moves to trash rather than evaporating, its open change — the commit it was holding — stays pinned by that timeline and is named on the way out, and the tip stays pinned by the operation. Nothing local is lost, nothing goes to `refs/stash`, there is no merged-check to argue with, and `ff undo` brings the branch and its timeline back.

The branch's operations themselves stay on the log either way; what goes is the way in through this name.

### Pruning

When a pull request merges and the forge deletes the branch, the local branch stays. `--prune` deletes every local branch whose shared copy is gone, in one operation, after a fetch of its own. Gone has three parts, all required: the branch has an upstream configured, its tracking ref is absent, and fufu has a record that the copy once stood — the tip it last showed you, or the one it last pushed. Without the record the shape is a fresh clone's, a copy never created, and the branch is unpublished, not gone.

A branch whose tip holds commits the copy never held is kept and named, with the count: that is work the copy's deletion did not take, and there is no `--force` — `ff branch -d <name>` is the verb for deleting one branch on purpose. The branch you are on, one checked out in another worktree, and one holding a rewrite are kept and named too.

A branch stacked on a pruned one is re-aimed at what the pruned branch sat on, the way `ff fold` re-aims them, and the report says so inline: `beta (delta now sits on main)`. The whole prune is one operation, so one `ff undo` brings every branch back with its timeline, its parent link, and its tracking section, and `ff status` on one then says its copy is gone, as before. `--dry-run` (`-n`) says what would go and what would be kept and writes nothing; `--no-fetch` prunes from the tracking refs as they stand.

Bare `ff branch` says how many are gone, with this flag as the way out. `fufu.pruneGone` lets `ff pull` do the same inside its run.

### A published branch

There is more than the name: a copy on the remote, and a tracking ref and upstream pointing at it. A plain delete leaves all three standing and says so.

`--shared` deletes the copy too, under a lease — only if it still stands where you last saw it, which is fufu's own record and not the tracking ref, so a fetch behind fufu's back does not refresh it — and takes the tracking ref and upstream down with it. A copy that moved since is refused before anything is deleted. That half left the machine: the branch still comes back, and the copy does not.

## Examples

```
ff branch                        what exists, and what is still anonymous
ff branch --all                  every remote branch too, unbounded
ff branch spike                  a branch at trunk, without moving there
ff branch spike main~2           the same, at a revision
ff branch spike @                at the commit under the open change
ff branch -d old-experiment      remove it (undoable)
ff branch -d spike --shared      the copy on the remote goes too
ff branch --prune                delete every branch whose shared copy is gone
ff branch --prune -n             say which would go, and write nothing
ff describe -b unicode-cleanup   name the branch you are on
```
