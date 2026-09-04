# ff branch

Bookkeeping for lines of work: [`ff branch list`](branch-list.md) says what exists and [`ff branch delete`](branch-delete.md) takes one away. Bare `ff branch` is the list. `ff br` is the short spelling, and `ff bookmark` is jj's name for the same verb.

Naming is not here. [`ff describe -b <name>`](describe.md) names the branch you are on, on the same axis as -m — one verb for saying what work is, whether the subject is the change's description or the branch's name.

## Usage

```
Usage: ff branch [OPTIONS] [COMMAND]

Commands:
  list    Named branches and anonymous ones, kept apart
  delete  Delete a branch — its timeline moves to trash, and `ff undo` is enough
  help    Print this message or the help of the given subcommand(s)

Options:
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
ff branch                        what exists, and what is still anonymous
ff branch delete old-experiment  remove it (undoable)
ff describe -b unicode-cleanup   name the branch you are on
```
