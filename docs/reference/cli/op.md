# ff op

Inspect and recover recorded operations: snapshots, fufu commands, and observed outside changes. Choose a subcommand; `ff op` alone requires one. For everyday recovery, [`ff history`](history.md) shows undo steps and [`ff undo`](undo.md) steps back without an ID.

## Usage

```
Usage: ff op [OPTIONS] <COMMAND>
```

## Examples

```sh
ff op log                       # Recorded operations, newest first
ff op show @                    # Inspect the live tip
ff op diff '@^' @               # Compare the last two snapshot trees
ff op restore '@~3'             # Return to three operations ago
ff op trim -n                   # Preview retention
```

## Options

```
Commands:
  log      List recorded operations, including captures, newest first
  show     Show an operation's ref transitions and file diffstat
  diff     Compare the recorded file trees of two operations
  restore  Restore this worktree's recorded local state at an operation
  revert   Invert an operation's ref transitions if those refs have not moved
  trim     Drop operations past the retention cutoff (fufu.keep, 90d)
  help     Print this message or the help of the given subcommand(s)

Options:
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

## Choosing a subcommand

- [`ff op log`](op-log.md) lists individual operations, including captures.
- [`ff op show`](op-show.md) shows one operation, its ref transitions, and file changes.
- [`ff op diff`](op-diff.md) compares two recorded file trees.
- [`ff op restore`](op-restore.md) restores the current worktree's recorded local state.
- [`ff op revert`](op-revert.md) inverts one operation's ref transitions, only if those refs have not moved again. It does not restore files.
- [`ff op trim`](op-trim.md) removes operations older than the retention window.

## Operation addresses

Operation IDs are hexadecimal. These subcommands and `--at-op` read operations; revision arguments read commits or k–z change IDs. `@` here means the live operation tip, not the open change. `@^` follows one predecessor and `@~3` follows three. Readers attempt capture first, so the tip can include this invocation's snapshot.

See [Revisions and IDs](../revisions.md#operation-expressions) for operation sets and past-state reads. Recovery follows the current worktree's retained chain, subject to worktree guards; it cannot rewind another chain or reverse a remote update.
