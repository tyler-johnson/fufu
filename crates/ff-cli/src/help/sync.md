Line every local branch up with both things it answers to: the base beneath it and the remote copy of itself. Fetch once, take in whatever arrived, replay onto the base. One verb for both, because reconciling with either is the same replay.

Nothing leaves the machine. Everything sync does is recorded and undoable, which is the whole reason it stops here — `ff publish` is the outgoing half, and it is a verb you type on purpose because a push cannot be taken back. Sync names what is waiting and leaves it.

Whose divergence it is decides what happens. Divergence this run's fetch created is somebody else's, and your commits replay on top of theirs. Divergence that was already there is yours only if fufu's own operation log accounts for every commit of it — as a rewrite it recorded, or as one it dropped as empty — and then there is nothing to take in and ff publish is what sends it. Commits the log does not recognize are somebody else's however they arrived, and they replay too.

Either replay can conflict. One that does holds that branch: nothing is written there, the run goes on to the next branch, and `ff resolve` on that branch picks it up. On any one branch, the first axis that conflicts leaves the other alone.

Sync runs over the whole repository. Every local branch gets both axes: the remote axis for every branch first, then the base axis parent before child, each replay cascading into the branches stacked above it the way `ff restack` does. A branch behind its shared copy fast-forwards, one standing exactly where the tracking ref stood before the fetch follows wherever the remote went, force-push included, and one that diverged gets the same rule as the branch you are standing on. Only a branch tracking the remote this run fetched from gets a remote axis; with `--no-fetch`, or a branch tracking another remote, the branch you are standing on is the only one whose shared copy is read, and the rest get the base axis alone. A branch checked out in another worktree, one already holding a rewrite, one whose shared copy this repository published and then undid, and one whose replay would touch a merge or shares no history with its base is named and left where it stands. Only the branch you are standing on carries a working tree, so the others move as refs and objects and touch no file. The whole run is one operation, so one `ff undo` takes every branch and the working tree back.

The report says what happened to the branch you are standing on first, then one block per other branch that did something: its name on a line of its own, and under it what moved, what held, and what was skipped. A branch with nothing to say prints nothing, so a repository of up-to-date branches still reads `nothing to sync`. With `--json`, the other branches are the `branches` array, one row per branch tagged `Synced`, `Elsewhere`, or `Held`; a `Synced` row carries its `remote` and `base` axes, tagged the same way, and `files` and `still_open` on the report describe the run's one working-tree write. The exit is 3 when a hold stands on any branch; when that branch is not the one you are standing on, the last line names it as the branch to switch to before `ff resolve`.

## Examples

```
ff sync                        fetch, line each branch up with base and remote
ff sync --no-fetch             reconcile with what you already have
ff publish                     send it, once it lines up
```
