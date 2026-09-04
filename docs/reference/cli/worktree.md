# ff worktree

Worktrees are how parallel work gets parallel trees: one checkout per line of work, each standing on a branch of its own. `add` makes one, `remove` takes one away, and bare `ff worktree` is the list. `ff workspace` is jj's name for it, and an alias here.

A worktree's operation chain is keyed by the worktree, so each one has its own log, its own undo, and its own lock.

## Usage

```
Usage: ff worktree [OPTIONS] [COMMAND]

Commands:
  list    Every worktree here, and every chain whose worktree is gone
  add     Make a worktree: a second checkout of this repository, with its own log
  remove  Take a worktree away, capturing what it holds first
  help    Print this message or the help of the given subcommand(s)

Options:
      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --json
          Emit machine-readable JSON

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff worktree              the live worktrees, and the gone chains
ff worktree list         the same, spelled out
ff worktree add bay      make one, on a branch of its own
ff worktree remove bay   take one away; its work is captured first
ff branch list           the branches those worktrees stand on
```
