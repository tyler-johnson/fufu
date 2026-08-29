Worktrees are how parallel work gets parallel trees: one checkout per line of work, each standing on a branch of its own. `add` makes one, `remove` takes one away, and bare `ff worktree` is the list.

A worktree's operation chain is keyed by the worktree, so each one has its own log, its own undo, and its own lock.

## Examples

```
ff worktree              the live worktrees, and the gone chains
ff worktree list         the same, spelled out
ff worktree add bay      make one, on a branch of its own
ff worktree remove bay   take one away; its work is captured first
ff branch list           the branches those worktrees stand on
```
