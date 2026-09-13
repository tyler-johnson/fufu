# ff evolog

Show the recorded evolution of one change, newest first. With no revision, show snapshots of the open change `@`. For a recorded commit, show operations that produced versions of the same change. `ff ev` is the short spelling.

## Usage

```
Usage: ff evolog [OPTIONS] [rev]
```

## Examples

```sh
ff evolog                       # Open change's snapshots
ff evolog HEAD~2                # An earlier change's evolution
ff evolog -n 0                  # All matching rows
ff evolog -p                    # Include each capture's patch
```

## Options

```
Arguments:
  [rev]
          Change ID, commit SHA, or revision to inspect; `@` when omitted

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

  -p, --patch
          Include patches for capture rows; operation rows have no patch

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

## Choosing a change

The revision can be a branch, commit SHA, or change ID from [`ff log`](log.md). `@` is the open change; `@^` is HEAD. See [Revisions and IDs](../revisions.md#commit-shas-and-change-ids). `--at` and `--at-op` are declared but currently refused here.

This command attempts a snapshot before reading. On a dirty tree, its newest row may be the capture taken by this invocation. Each capture holds a worktree snapshot; use its operation ID with [`ff restore <path> --at-op <id>`](restore.md) to recover files within snapshot coverage and retention limits.

## Reading the rows

For a recorded change, operation rows search all worktree chains for versions carrying its `change-id`: commit, reword, restack, absorb, and pull replay operations. A `captures` divider introduces the snapshots behind its commit. A commit with no stored change ID gets matching captures from this chain when available.

Rows display hexadecimal operation IDs. Their bold prefixes distinguish retained operations, not changes. `-p` prints a capture's patch against the preceding capture on its branch; operation rows name the commit produced and have no patch beneath them.

An outside rebase or cherry-pick can drop the `change-id` header. The rewritten commit then has a derived ID and does not continue the old evolution history. Use [`ff op log`](op-log.md) for all recorded operations, or [`ff history`](history.md) for undo steps.
