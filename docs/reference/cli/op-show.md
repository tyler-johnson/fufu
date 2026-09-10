# ff op show

One operation in full: what ran, when, on which branch, what it moved, and the diffstat of the worktree it carries against the operation before it. Bare `ff op show` reads `@`, the newest.

Every operation has a tree, which is what makes this uniform — a capture and a close are read the same way, and differ only in whether there are ref transitions to list.

-p puts the patch under the diffstat rather than in place of it: the same unified diff [`ff diff`](diff.md) prints, for the operation instead of the tree.

## Usage

```
Usage: ff op show [OPTIONS] [op]

Arguments:
  [op]
          The operation; `@` (the newest) when omitted

Options:
  -p, --patch
          Print the patch under the diffstat, not just the counts

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

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
ff op show                     the newest operation
ff op show @^                  the one before it
ff op show 9dfd5e5d            by id
ff op show -p @                what it changed, with content
ff op show --json              the same, for machines
```
