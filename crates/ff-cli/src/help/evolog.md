Show the recorded evolution of one change, newest first. With no revision, show snapshots of the open change `@`. For a recorded commit, show operations that produced versions of the same change. `ff ev` is the short spelling.

## Examples

```sh
ff evolog                       # Open change's snapshots
ff evolog HEAD~2                # An earlier change's evolution
ff evolog -n 0                  # All matching rows
ff evolog -p                    # Include each capture's patch
```

### Options

### Choosing a change

The revision can be a branch, commit SHA, or change ID from `ff log`. `@` is the open change; `@^` is HEAD. See [Revisions and IDs](../revisions.md#commit-shas-and-change-ids). `--at` and `--at-op` are declared but currently refused here.

This command attempts a snapshot before reading. On a dirty tree, its newest row may be the capture taken by this invocation. Each capture holds a worktree snapshot; use its operation ID with `ff restore <path> --at-op <id>` to recover files within snapshot coverage and retention limits.

### Reading the rows

For a recorded change, operation rows search all worktree chains for versions carrying its `change-id`: commit, reword, restack, absorb, and pull replay operations. A `captures` divider introduces the snapshots behind its commit. A commit with no stored change ID gets matching captures from this chain when available.

Rows display hexadecimal operation IDs. Their bold prefixes distinguish retained operations, not changes. `-p` prints a capture's patch against the preceding capture on its branch; operation rows name the commit produced and have no patch beneath them.

An outside rebase or cherry-pick can drop the `change-id` header. The rewritten commit then has a derived ID and does not continue the old evolution history. Use `ff op log` for all recorded operations, or `ff history` for undo steps.
