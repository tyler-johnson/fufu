# ff restack

Replay the current branch's commits onto its base branch's latest tip. The base is its recorded parent, or trunk when none is recorded. Name another branch to restack it, or use `--onto` to change the selected branch's base. `ff rebase` is an alias.

## Usage

```
Usage: ff restack [OPTIONS] [branch]
```

## Examples

```sh
ff restack                      # Update this branch from its base
ff restack feature              # Restack another branch
ff restack --onto release-1.2   # Choose a new base
ff restack --onto origin/main   # Use a remote-tracking base
```

## Options

```
Arguments:
  [branch]
          Branch to restack; defaults to the current branch

Options:
      --onto <branch>
          Base to replay onto; recorded as this branch's new parent

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Selecting a base and side effects

`--onto` records the new base and replays onto it. Local branches and remote-tracking branches such as `origin/main` are accepted. Restacking another branch leaves this worktree's files alone unless the cascade reaches the current branch.

Replay is local. The CLI can auto-fetch first and run maintenance afterward. `--no-fetch` skips the fetch; passive update behavior has separate configuration.

## Conflicts and dependent branches

A conflicting primary replay leaves that branch's tip and files at their pre-replay state and records a held rewrite. Captures, metadata, and successful replays elsewhere may still be written. [`ff resolve`](resolve.md) opens a held rewrite. A held restack already on the branch is dropped by `--onto` another base and named, cleared by a replay onto its base, and kept otherwise; a held absorb, lift, or done refuses the restack.

Dependent branches replay parent before child in the same operation. A downstream conflict holds that branch and leaves its dependents alone. Branches checked out elsewhere or already held are skipped and named. Switch to a held branch to resolve it. The exit is 3 when a primary or downstream replay holds. One [`ff undo`](undo.md) takes back the restack and cascade.

## Replay selection and report

The replay carries the branch's own commits. A source change ID already present on the target is dropped as superseded, without merging its old content. For commits without stored change IDs, the fork point uses the target's reflog to exclude stale history, including when `--onto` selects a new target.

A branch inside the replay range with no commits of its own stays put and is named. Output lists what followed, held, and was skipped; JSON carries the dependent-branch results in `cascade`.
