# ff op diff

What changed in the worktree between two operations. Both are operation ids; the second defaults to `@`, so a single argument reads "from there to now".

This compares the trees two operations carry, not their ref transitions — adjacent operations can sit on different branches, and the diff across that seam reads as the whole worktree being replaced, which is literal rather than wrong.

-p puts the patch under the diffstat, the same unified diff `ff diff` prints.

## Usage

```
Usage: ff op diff [OPTIONS] <a> [b]

Arguments:
  <a>
          The older operation

  [b]
          The newer operation; `@` when omitted

Options:
  -p, --patch
          Print the patch under the diffstat, not just the counts

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
ff op diff @^ @                what the newest operation changed
ff op diff kqzm                from that operation to now
ff op diff kqzm kwzq           between two of them
ff op diff -p @^ @             with content, not just counts
```
