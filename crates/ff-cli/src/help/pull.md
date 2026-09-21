Update the current branch from its base branch and its remote copy. Pull fetches first, incorporates remote work, and replays local commits when needed. It also updates the selected branch's local base branches, down to trunk. `ff sync` is an alias.

## Examples

```sh
ff pull                         # Update this branch and its local bases
ff pull side                    # Select side instead
ff pull a b                     # Select two branches, with one fetch
ff pull --all                   # Select every local branch
ff pull -n                      # Preview local updates; still fetches
ff pull -n --no-fetch           # Preview using existing tracking refs
ff pull --resolve               # Open the session if this branch conflicts
ff push                         # Send this branch after reviewing it
```

### Options

### Which branches

With no names, pull selects the current branch. Names select one or more branches instead; `--all` selects every local branch. Unique local branch prefixes are accepted. Unknown or ambiguous names are refused before fetching.

Each selected branch brings its local base branches into the run, down to trunk. Their remote copies are incorporated first, so a teammate's change on main can reach your branch through local main, even when trunk is configured as `origin/main`.

Dependent branches above a selected branch are not themselves selected. They follow a replay beneath them, but their own remote copies are not incorporated unless you name them or use `--all`. A non-current branch that only fast-forwards moves its ref without cascading to dependents.

### Remote and base updates

If a local branch has not changed since fufu last recorded seeing its remote copy, it follows that copy, including a force-push. Otherwise new remote work is incorporated and local commits replay above it. Remote commits recognized as old versions of local work are left for `ff push` to replace. An ahead branch is reported without sending anything.

Only branches tracking the fetched remote receive this remote update. With `--no-fetch`, or for a branch tracking another remote, only the current branch's remote copy is considered. The base update still runs without a remote: if the base moved, local commits replay onto it and dependent branches follow parent before child.

How the base update takes a moved base in follows `fufu.pull`. `auto`, the default, replays a straight line and leaves a branch standing once its commits hold any merge. `replay` replays the branch's commits onto the base, flattening a merge of the base the way any replay does. `merge` leaves a branch behind its base standing. `fufu.<pattern>.pull` chooses per branch: `ff config pull` lists the rows. A branch beneath its base with nothing of its own fast-forwards under every value.

A deleted remote copy is reported while its local branch remains. With `fufu.pruneGone` enabled, eligible branches are pruned first using `ff branch --prune`'s rules, including protection for unpublished commits and reassignment of dependents. Pruning is part of the same undoable operation. The setting defaults to false.

### Conflicts and recovery

A conflicting replay holds that branch without landing its new tip or files. The run continues with other branches; dependents of the held branch stay put. Captures and hold metadata may still be written. Switch to the named branch and run `ff resolve` to continue. The exit is 3 if any branch holds.

`--resolve` opens the current branch's resolution session in the same run, still with exit 3. Fix the marked files and run `ff done`; repeat if another round appears. Other branches stay held without opening a session, and a dry run opens none. Use `ff config onConflict resolve` to make this the default, or `--no-resolve` to stop at the hold for one run.

`ff done --abandon` closes the session and keeps the hold; `ff resolve --abandon` drops both. Opening during pull adds two operations after the pull's own operation. Three `ff undo` calls reverse the switch, the session creation, then the pull.

Branches checked out in another worktree, already holding a rewrite, or sharing no history with their base are skipped and named. A branch its pull policy leaves standing is reported behind its base with the policy that decided it and where it was set; `ff restack` replays it regardless, and `ff resolve` on it takes the base in the way the branch already does.

Local branch and working-copy changes form one operation, including cascades and pruning. One `ff undo` reverses them. Only the current branch has files written in this worktree; other selected branches move as refs and objects. Fetched objects, tracking refs, and tags are separate from these undoable changes. No remote branch update is sent.

### Dry runs and network effects

`--dry-run` (`-n`) plans every selected branch and reports what would fast-forward, replay, hold, or be skipped. It moves no local branch, writes no worktree files, takes no pull capture, and records no hold or replay operation. The exit is 3 if a branch would hold.

Fetching still writes objects, remote-tracking refs, and tags, and prunes tracking refs for deleted remote branches. Add `--no-fetch` to use existing refs. Successful invocations can still run automatic trimming and update maintenance in either form.

### Report and JSON

The current branch is reported first when selected, then other branches that changed, held, or were skipped. An idle run prints `nothing to pull`.

With `--json`, the other selected branches appear in `branches`, tagged `Pulled`, `Elsewhere`, or `Held`. A `Pulled` row has `remote` and `base` results. The top-level `remote` and `base` describe the current branch and read `NotNamed` when it was not selected. `files` and `still_open` describe the one working-copy write. In a dry run, `undo` is null and `files` is the projected count; `dry_run` describes the replay preview, not the absence of all writes.
