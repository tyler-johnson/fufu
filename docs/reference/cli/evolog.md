# ff evolog

Every operation on a change, newest first — the drill-in behind the letters column in [`ff log`](log.md). Bare, or on `@`, it is the open change: each row is a capture, a whole worktree, and [`ff restore --at-op <id>`](restore.md) brings any of them back. This is where a lost hour is found. `ff ev` is the short spelling.

Because fufu captures before it works, the newest row is often this command's own capture, taken a moment ago when it found the tree dirty. That is intended.

On a revision — a change id, a prefix of one, a sha — it is that change's history: every operation, on every worktree's chain, that produced a commit carrying its id — the close, a reword, a restack, an absorb, a pull's replay — with the commit each produced, and under a `captures` divider the captures behind the close: the work the commit closed. A commit fufu did not close has no header and no operations; it gets the captures on this chain that match it, when any do.

The thread is the `change-id` header. A rebase or cherry-pick run outside fufu drops it, so a commit rewritten behind fufu's back comes back with a derived id and its history starts over there; jj has the same limitation.

Ids are spelled in the letters k–z; the bold prefix on a capture row is the shortest one [`ff op`](op.md) and `--at-op` resolve unambiguously, and on an operation row the same.

-p prints each capture row's patch under it — what that one capture changed, measured against the capture before it on its branch. Operation rows name what they produced and print nothing under it.

## Usage

```
Usage: ff evolog [OPTIONS] [rev]

Arguments:
  [rev]
          The change to drill into: a change id, a sha, any revision; `@` when omitted

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

  -p, --patch
          Print each row's patch under it — what that operation changed

      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff evolog                      the open change's captures
ff evolog nyrszqtk             a change's operations, by the id ff log prints
ff evolog HEAD~2               the same, by revision
ff evolog -n 0                 all of them
ff evolog -p                   each row with what it changed, in full
ff restore src/ --at-op <id>   pull a directory back from one
```
