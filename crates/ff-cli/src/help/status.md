Show the current branch, its base and remote relationship, and uncommitted file changes. Files are listed with insertion and deletion counts against the commit below the open change. `ff st` is the short spelling.

## Examples

```sh
ff status                       # Branch and file summary
ff status --json                # The same state for scripts
ff status --no-fetch            # Use existing remote-tracking refs
ff diff                         # Read the file changes as a patch
```

### Options

### Files and outside changes

The file summary includes eligible untracked content as well as tracked edits. `ff diff` shows the corresponding patch. Snapshot exclusions, including ignored untracked files and oversized content, still apply.

The CLI attempts capture and reconciles Git ref changes made outside fufu. Status reports those changes until the next fufu operation. `ff op show` lists an operation's individual ref transitions. A held rewrite or parked-arrival conflict is reported here with the next recovery command.

### Fetching and JSON

Remote counts use tracking refs. Automatic fetching can refresh them on `fufu.autoFetch`'s cadence, ten minutes by default, pruning deleted remote branches. `--fetch` requests it now; `--no-fetch` skips it. `ff pull` updates local branches. Automatic trimming and update maintenance can also run.

JSON includes the checkout root, worktree identity, branch base and distance, remote, and the last operation with its session. That last-operation field excludes the status invocation's own capture. `--at` and `--at-op` are declared but currently refused here.
