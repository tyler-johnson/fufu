Step forward after `ff undo` or `ff op restore`. It takes no argument. Repeat it to follow the available redo path in the current worktree; `ff history` shows that path above `@`.

## Examples

```sh
ff undo                         # Step back
ff redo                         # Return forward
ff history                      # See remaining steps
```

### Options

### When redo stops

New work after undo forks the operation history and ends the offered redo path. The older records are retained rather than truncated. Their operation IDs can still be used with `ff op restore` until retention removes them.

Redo has the same local scope as undo: recorded refs, HEAD, index, and files for this worktree, subject to worktree guards. It cannot reproduce uncaptured content or change a remote.
