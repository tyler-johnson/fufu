# ff undo

Undo one recorded local step in the current worktree, restoring local refs, HEAD, index, and files together. It takes no argument. Repeat it to move farther back through retained history; [`ff history`](history.md) shows the available steps.

## Usage

```
Usage: ff undo [OPTIONS]
```

## Examples

```sh
ff history                      # Inspect available undo steps
ff undo                         # Step back once
ff undo                         # Step back again
ff redo                         # Step forward again
ff op restore '@~3'             # Choose an operation instead of a step
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

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## What one step means

Adjacent captures with the same session label form one undo step. Each recorded command operation is its own step, so a switch and a commit take two undos. Opening a rewrite resolution also takes two operations: one undo returns from the session, another removes it. Landing or abandoning that session is one operation.

## Recovery limits and redo

Undo follows this worktree's operation chain, subject to worktree guards. It cannot rewind another worktree's chain, reverse remote pushes, or recover bytes that were never captured. Retention bounds how far it can go.

Undo moves the operation pointer instead of appending. The state left behind, including the snapshot taken before undo, remains reachable. [`ff redo`](redo.md) follows it forward while that path is still offered. New work after undo forks history; retained older operations can still be addressed with [`ff op restore`](op-restore.md).
