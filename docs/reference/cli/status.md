# ff status

Show the current branch, its base and remote relationship, and uncommitted file changes. Files are listed with insertion and deletion counts against the commit below the open change. `ff st` is the short spelling.

## Usage

```
Usage: ff status [OPTIONS]
```

## Examples

```sh
ff status                       # Branch and file summary
ff status --json                # The same state for scripts
ff status --no-fetch            # Use existing remote-tracking refs
ff diff                         # Read the file changes as a patch
```

## Options

```
Options:
      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Files and outside changes

The file summary includes eligible untracked content as well as tracked edits. [`ff diff`](diff.md) shows the corresponding patch. Snapshot exclusions, including ignored untracked files and oversized content, still apply.

The CLI attempts capture and reconciles Git ref changes made outside fufu. Status reports those changes until the next fufu operation. [`ff op show`](op-show.md) lists an operation's individual ref transitions. A held rewrite or parked-arrival conflict is reported here with the next recovery command.

## Fetching and JSON

Remote counts use tracking refs. Automatic fetching can refresh them on `fufu.autoFetch`'s cadence, ten minutes by default, pruning deleted remote branches. `--fetch` requests it now; `--no-fetch` skips it. [`ff pull`](pull.md) updates local branches. Automatic trimming and update maintenance can also run.

JSON includes the checkout root, worktree identity, branch base and distance, remote, and the last operation with its session. That last-operation field excludes the status invocation's own capture. `--at` and `--at-op` are declared but currently refused here.
