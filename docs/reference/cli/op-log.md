# ff op log

List recorded operations, newest first, including captures. By default, show the last 25 entries on the current worktree's live chain. Use [`ff history`](history.md) to see grouped undo steps instead.

## Usage

```
Usage: ff op log [OPTIONS] [revset]
```

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

## Options

```
Arguments:
  [revset]
          Operations to show, as a revset over the operation log

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --json
          Emit machine-readable JSON

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

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

## IDs and expressions

Rows have hexadecimal operation IDs. The bold prefix distinguishes retained history, including abandoned entries and other worktrees. Later operations can make a copied prefix ambiguous; use more characters then.

The positional argument is an operation set. `@` means the live operation tip; `@^` is its predecessor and `::@` is its ancestry. Operators match the revision-set language used by [`ff log`](log.md), but select operations. Functions include `on_branch()`, `session()`, `kind()`, `latest()`, `heads()`, and `roots()`, each with one argument. Bare patterns match substrings; `exact:` requests an exact match. See [operation expressions](../revisions.md#operation-expressions).

## Captures and past-state reads

The CLI attempts capture before reading, so a dirty tree can put this invocation's snapshot at the top.

`--at-op` and `--at` bound output at a past operation. Without an expression, the walk starts there. With one, the expression is evaluated on the live log and then intersected with the bound's ancestry. Thus `ff op log '@' --at-op '@^'` returns no rows. See [past-state reads](../revisions.md#paths-sources-and-past-state-reads).
