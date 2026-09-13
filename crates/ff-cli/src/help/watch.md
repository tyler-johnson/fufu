Stream operation-history changes as one JSON object per line. By default, watch the current worktree until interrupted. It runs in the foreground and writes no captures, refs, or reconciliation records. Ctrl-C stops it.

## Examples

```sh
ff watch -n 1                   # Print the opening state and exit
ff watch                        # Follow this worktree
ff watch --all                  # Follow every worktree
ff watch --kind op              # Filter to command operations
ff watch --session flight-3     # Filter to a session label
```

### Options

### Filters and replay

`--kind` accepts capture, op, foreign, or note. `--session` filters by session. `--since <op>` replays from a retained operation before following new events; it accepts hexadecimal operation IDs, prefixes, and `@` predecessor suffixes. It cannot be combined with `--all` because an address belongs to one chain.

`-n` counts emitted events, including the opening event; zero means unlimited. Output is always JSON, regardless of `--json`.

### Events and completion

Each stream starts with `start`, naming its initial tip. `landed` reports an appended operation, `stepped-back` a backward pointer move, `forked` new work after undo, and `rewritten` a rewritten chain such as a trim.

A single-worktree rewrite invalidates the stream's anchor: it emits `rewritten` and exits 1. Under `--all`, only that chain re-anchors and the stream continues. The all-worktree stream emits a start for each chain, discovers new worktrees, and retains removed worktree chains. Every line identifies its worktree in both modes.

### Write-ahead records

An operation is recorded before its described mutation is applied. An event therefore does not prove that the subsequent ref or file write completed. `ff op log` reads those same records.
