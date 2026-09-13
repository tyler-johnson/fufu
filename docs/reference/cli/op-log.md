# ff op log

Every operation, newest first, wearing the ids the [`ff op`](op.md) verbs take. Captures are in there too, and they outnumber verb operations by more than an order of magnitude, so narrowing is the expression's job — `ff op log 'kind(op)'`. Where you can go *back* to is a different question, and [`ff history`](history.md) is the verb for it.

The bold prefix on each ID distinguishes it across retained operation history, including abandoned entries and other worktrees. New operations can make an old prefix ambiguous; copy more of the ID if needed.

This verb attempts a snapshot before reading, so on a dirty tree the newest row can be this command's own capture — the same timing [`ff evolog`](evolog.md) has.

## The expression

The argument is the set language over operations: the same operators as [`ff log`](log.md), reading the other address space, and positional the way an operation id is positional in [`ff op show`](op-show.md).

Ancestry follows the log, so `@^` is the operation before the newest and `::@` is the whole live log. Operations have on_branch(), session() and kind(), and share latest(), heads() and roots() with revisions. All take one argument. Use `session(exact:nightly)` for an exact session label; a bare pattern is a substring match. See [operation expressions](../revisions.md#operation-expressions) for the complete syntax.

--at-op and --at bound the output at a past operation. Without an expression, the walk starts there. With an expression, it is evaluated against the live log, then filtered to entries at or before the bound: `@` still means the live tip, so `ff op log '@' --at-op '@^'` returns no rows. See [past-state reads](../revisions.md#paths-sources-and-past-state-reads).

## Usage

```
Usage: ff op log [OPTIONS] [revset]

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
ff op log                      the last 25 operations, every kind
ff op log 'kind(op)'           verb operations only
ff op log 'kind(capture)'      the machine-rate rows alone
ff op log 'session(nightly)'   one session's operations
ff op log '~on_branch(main)'   everything that happened elsewhere
ff log -r 'base(@)'            the commit the newest operation ran on
ff op log --at 2h              the log as it read two hours ago
```
