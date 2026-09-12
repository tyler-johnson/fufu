# ff op

The operation log as objects. Every capture and every fufu mutation lands on one log at refs/fufu/ops, and this is the family that reads and moves it: `log` lists, `show` and `diff` read, `restore` rewinds the whole repository to one, `revert` inverts a single one leaving later work standing, and `trim` is the family's delete, dropping what lies past the keep window.

Operation ids are hex, the same as commits, and the slot decides which space a prefix is read in: these verbs and `--at-op` read operations, `-r` and `--from` read revisions. Letters are always a change id. `@` is the newest operation, and git's own first-parent suffixes work on it — `@^` is the one before, `@~3` three back — because an operation's first parent *is* the operation before it.

[`ff undo`](undo.md) is the everyday shortcut for [`ff op restore`](op-restore.md), argument-free and repeatable; most work never needs the long form.

## Usage

```
Usage: ff op [OPTIONS] <COMMAND>

Commands:
  log      Every operation, newest first, with the ids these verbs take
  show     Show one operation: what it was, what it moved, what it holds
  diff     Compare the worktrees two operations carry
  restore  Rewind the whole repository to an operation
  revert   Invert one operation, leaving later work standing
  trim     Drop operations past the retention cutoff (fufu.keep, 90d)
  help     Print this message or the help of the given subcommand(s)

Options:
      --json
          Emit machine-readable JSON

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
ff op log                      what has happened, newest first
ff op show @                   what the newest operation did
ff op diff @^ @                what changed across it
ff op restore 9dfd5e5d         rewind the whole repository there
ff undo                        the same move, one run at a time
ff op trim -n                  what retention would drop, nothing written
```
