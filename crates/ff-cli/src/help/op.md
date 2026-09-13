Inspect and recover recorded operations: snapshots, fufu commands, and observed outside changes. Choose a subcommand; `ff op` alone requires one. For everyday recovery, `ff history` shows undo steps and `ff undo` steps back without an ID.

## Examples

```sh
ff op log                       # Recorded operations, newest first
ff op show @                    # Inspect the live tip
ff op diff '@^' @               # Compare the last two snapshot trees
ff op restore '@~3'             # Return to three operations ago
ff op trim -n                   # Preview retention
```

### Options

### Choosing a subcommand

- `ff op log` lists individual operations, including captures.
- `ff op show` shows one operation, its ref transitions, and file changes.
- `ff op diff` compares two recorded file trees.
- `ff op restore` restores the current worktree's recorded local state.
- `ff op revert` inverts one operation's ref transitions, only if those refs have not moved again. It does not restore files.
- `ff op trim` removes operations older than the retention window.

### Operation addresses

Operation IDs are hexadecimal. These subcommands and `--at-op` read operations; revision arguments read commits or k–z change IDs. `@` here means the live operation tip, not the open change. `@^` follows one predecessor and `@~3` follows three. Readers attempt capture first, so the tip can include this invocation's snapshot.

See [Revisions and IDs](../revisions.md#operation-expressions) for operation sets and past-state reads. Recovery follows the current worktree's retained chain, subject to worktree guards; it cannot rewind another chain or reverse a remote update.
