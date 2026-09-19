# ff op restore

Restore the current worktree's recorded local state at an operation: local refs, HEAD, index, and files together. An operation address is required. This follows the current worktree's chain, subject to worktree guards, and cannot reverse a remote update.

## Usage

```
Usage: ff op restore [OPTIONS] <op>
```

## Examples

```sh
ff history                      # Choose a retained recovery point
ff op restore '@~3'             # Restore three operations ago
ff redo                         # Step forward after the rewind
```

## Options

```
Arguments:
  <op>
          Operation whose recorded local state should be restored

Options:
      --force
          Rewind to what remains even if parts were trimmed

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

## Choosing a recovery point

Use a hexadecimal operation ID from [`ff history`](history.md) or [`ff op log`](op-log.md), or `@` with predecessor suffixes. `@` is the live operation tip; `@^` is the preceding operation. See [operation addresses](../revisions.md#operation-expressions). [`ff undo`](undo.md) chooses one grouped undo step without an address.

Recovery requires retained snapshots and cannot recover uncaptured content or rewind another worktree's chain. For files alone, use [`ff restore <path> --at-op <id>`](restore.md) instead.

## Missing state and redo

`--force` restores what remains when parts of a recorded state have been trimmed, reporting missing pieces instead of refusing. It cannot recreate missing content.

Restore moves the operation pointer rather than appending a new operation. The state you leave remains reachable, so [`ff redo`](redo.md) can move forward until new work ends that redo path or retention removes it.
