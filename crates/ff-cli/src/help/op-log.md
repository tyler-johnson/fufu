List recorded operations, newest first, including captures. By default, show the last 25 entries on the current worktree's live chain. Use `ff history` to see grouped undo steps instead.

## Examples

```sh
ff op log                       # Last 25 entries of every kind
ff op log 'kind(op)'            # Fufu command operations
ff op log 'kind(capture)'       # Snapshots only
ff op log 'session(exact:nightly)'  # One exact session label
ff op log '~on_branch(main)'    # Entries from other branches
ff op log --at 2h               # Entries at or before two hours ago
ff log -r 'base(@)'             # Base commit of the live operation
```

### Options

### IDs and expressions

Rows have hexadecimal operation IDs. The bold prefix distinguishes retained history, including abandoned entries and other worktrees. Later operations can make a copied prefix ambiguous; use more characters then.

The positional argument is an operation set. `@` means the live operation tip; `@^` is its predecessor and `::@` is its ancestry. Operators match the revision-set language used by `ff log`, but select operations. Functions include `on_branch()`, `session()`, `kind()`, `latest()`, `heads()`, and `roots()`, each with one argument. Bare patterns match substrings; `exact:` requests an exact match. See [operation expressions](../revisions.md#operation-expressions).

### Captures and past-state reads

The CLI attempts capture before reading, so a dirty tree can put this invocation's snapshot at the top.

`--at-op` and `--at` bound output at a past operation. Without an expression, the walk starts there. With one, the expression is evaluated on the live log and then intersected with the bound's ancestry. Thus `ff op log '@' --at-op '@^'` returns no rows. See [past-state reads](../revisions.md#paths-sources-and-past-state-reads).
