Replay the current branch's commits onto its base branch's latest tip. The base is its recorded parent, or trunk when none is recorded. Name another branch to restack it, or use `--onto` to change the selected branch's base. `ff rebase` is an alias.

## Examples

```sh
ff restack                      # Update this branch from its base
ff restack feature              # Restack another branch
ff restack --onto release-1.2   # Choose a new base
ff restack --onto origin/main   # Use a remote-tracking base
```

### Options

### Selecting a base and side effects

`--onto` records the new base and replays onto it. Local branches and remote-tracking branches such as `origin/main` are accepted. Restacking another branch leaves this worktree's files alone unless the cascade reaches the current branch.

Replay is local. The CLI can auto-fetch first and run maintenance afterward. `--no-fetch` skips the fetch; passive update behavior has separate configuration.

### Conflicts and dependent branches

A conflicting primary replay leaves that branch's tip and files at their pre-replay state and records a held rewrite. Captures, metadata, and successful replays elsewhere may still be written. `ff resolve` opens a held rewrite.

Dependent branches replay parent before child in the same operation. A downstream conflict holds that branch and leaves its dependents alone. Branches checked out elsewhere, already held, or containing merges are skipped and named. Switch to a held branch to resolve it. The exit is 3 when a primary or downstream replay holds. One `ff undo` takes back the restack and cascade.

### Replay selection and report

The replay carries the branch's own commits. A source change ID already present on the target is dropped as superseded, without merging its old content. For commits without stored change IDs, the fork point uses the target's reflog to exclude stale history, including when `--onto` selects a new target.

A branch inside the replay range with no commits of its own stays put and is named. Output lists what followed, held, and was skipped; JSON carries the dependent-branch results in `cascade`.
