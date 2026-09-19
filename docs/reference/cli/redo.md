# ff redo

Step forward after [`ff undo`](undo.md) or [`ff op restore`](op-restore.md). It takes no argument. Repeat it to follow the available redo path in the current worktree; [`ff history`](history.md) shows that path above `@`.

## Usage

```
Usage: ff redo [OPTIONS]
```

## Examples

```sh
ff undo                         # Step back
ff redo                         # Return forward
ff history                      # See remaining steps
```

## Options

```
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

## When redo stops

New work after undo forks the operation history and ends the offered redo path. The older records are retained rather than truncated. Their operation IDs can still be used with `ff op restore` until retention removes them.

Redo has the same local scope as undo: recorded refs, HEAD, index, and files for this worktree, subject to worktree guards. It cannot reproduce uncaptured content or change a remote.
