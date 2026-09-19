# ff history

Show available undo and redo steps for the current worktree. By default, show 25 undo steps. Each row below `@` is one more [`ff undo`](undo.md); each row above it is one more [`ff redo`](redo.md).

## Usage

```
Usage: ff history [OPTIONS]
```

## Examples

```sh
ff history                      # Last 25 undo steps
ff history -n 0                 # Back to the earliest recovery point
ff history --json               # Steps as JSON
ff op show @                    # Inspect the live operation tip
```

## Options

```
Options:
  -n, --max-count <COUNT>
          Number of undo steps to show; 0 means unlimited
          
          [default: 25]

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

## Steps and operation IDs

Here `@` marks the current operation state. Adjacent captures in one session collapse into an undo step, with their count shown. Recorded command operations are separate steps. Opening a rewrite resolution takes two operations: one undo returns from the session, another removes it.

Rows carry hexadecimal operation IDs accepted by [`ff op show <id>`](op-show.md) and [`ff op restore <id>`](op-restore.md). [`ff op log`](op-log.md) lists individual recorded operations; [`ff log`](log.md) shows commits, and [`ff evolog`](evolog.md) shows one change's evolution. See [Revisions and IDs](../revisions.md#operation-expressions).

## Recovery limits

New work after undo forks the operation history and ends the offered redo path. The older operations remain addressable until retention removes them. Recovery requires retained captures; it cannot restore uncaptured bytes, another worktree's chain, or remote effects.
