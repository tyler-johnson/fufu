Show available undo and redo steps for the current worktree. By default, show 25 undo steps. Each row below `@` is one more `ff undo`; each row above it is one more `ff redo`.

## Examples

```sh
ff history                      # Last 25 undo steps
ff history -n 0                 # Back to the earliest recovery point
ff history --json               # Steps as JSON
ff op show @                    # Inspect the live operation tip
```

### Options

### Steps and operation IDs

Here `@` marks the current operation state. Adjacent captures in one session collapse into an undo step, with their count shown. Recorded command operations are separate steps. Opening a rewrite resolution takes two operations: one undo returns from the session, another removes it.

Rows carry hexadecimal operation IDs accepted by `ff op show <id>` and `ff op restore <id>`. `ff op log` lists individual recorded operations; `ff log` shows commits, and `ff evolog` shows one change's evolution. See [Revisions and IDs](../revisions.md#operation-expressions).

### Recovery limits

New work after undo forks the operation history and ends the offered redo path. The older operations remain addressable until retention removes them. Recovery requires retained captures; it cannot restore uncaptured bytes, another worktree's chain, or remote effects.
