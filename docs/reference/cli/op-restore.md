# ff op restore

Rewind the current worktree's recorded state to an operation: local refs, HEAD, the working copy and the index together, subject to worktree guards. This does not rewind another worktree's chain or a remote push.

It moves the log's pointer rather than appending, so what it steps off stays reachable and [`ff redo`](redo.md) walks back forward along it. Nothing is discarded and no entry is written saying you navigated — the log records work, not movement.

--force rewinds to what remains when parts of the recorded state have already been trimmed, naming each missing piece instead of refusing.

[`ff undo`](undo.md) is this verb without an argument, moving one run at a time.

## Usage

```
Usage: ff op restore [OPTIONS] <op>

Arguments:
  <op>
          The operation to land on

Options:
      --force
          Rewind to what remains even if parts were trimmed

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff op restore a1b2c3d4e5f6     land on that operation
ff op restore @~3              three operations back
ff op restore @ --force        what remains, after a trim took the rest
ff redo                        undo the rewind
```
