# ff restore

Files come back as they were somewhere else. Bare, that somewhere is the commit under the open change — the everyday "discard my edits to this file". --all restores the whole tree, including deleting files that were created since.

Three flags name a different source, one kind each, because a position argument has exactly one kind and a second kind takes a flag:

```
--from <rev>      a revision — a branch, a sha, any revset naming one
--at-op <op>      an operation, by its letters-spelled id
--at <time>       the operation current at a time (30m/2h/3d, or a date)
```

Only the worktree is written. The index, HEAD, and branches stay exactly as they are. Restore takes its own capture first, and that one is mandatory: if the pre-restore capture fails, nothing is written. So any restore is undone by another restore, or by `ff undo`.

## Usage

```
Usage: ff restore [OPTIONS] [path]...

Arguments:
  [path]...
          Paths to restore from the source

Options:
      --from <rev>
          Revision to restore from; without it, the commit under the change

      --all
          Restore the entire worktree to the source state

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
ff restore src/main.rs         discard edits: back to the commit below
ff restore --all --at 2h       the whole tree, as it stood two hours ago
ff restore docs/ --at-op kqzm  a directory, from one operation
ff restore src/ --from main~2  the same paths, from history instead
```
