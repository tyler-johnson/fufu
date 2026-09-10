Worktrees are how parallel work gets parallel trees: one checkout per line of work, each standing on a branch of its own. Bare `ff worktree` is the list, `ff worktree <path> [<branch>]` makes one, and `ff worktree -d <worktree>` takes one away. `ff workspace` is jj's name for it, and an alias here.

A worktree's operation chain is keyed by the worktree, so each one has its own log, its own undo, and its own lock.

### The list

The live worktrees first, with their checkouts and the branches they stand on, then the chains whose worktree is gone.

That second section is the earn: a chain lives in the shared ref namespace, so it outlives the checkout, and it is what keeps a deleted bay's work addressable. The tip op id on each row is what ff restore --at-op takes. Retention still ages those chains out on the ordinary fufu.keep cadence — surviving the worktree is not living forever.

### Adding

A second checkout of the same repository: one object store and one ref namespace are shared, and the working copy, the index, and HEAD are what is new.

The chain floor is laid as the worktree is made, so ff undo works there from the first command. A checkout written by hand gets its floor on its first fufu command instead, and undo in it is blind until then.

The branch is a name you give, or a new branch named after the directory when you do not say, or a minted name when that name is taken. A branch open in another worktree is refused: git allows a branch in one tree, and fufu enforces it.

### Removing

The capture comes first, into the worktree's own chain, and that is why there is no --force: git demands one for a dirty worktree because it has nowhere to put the work, and fufu has put it. The uncommitted work survives the removal, and the removal says where it went.

The chain stays behind, and ff worktree will show it under the chains whose worktree is gone — its tip is what ff restore --at-op takes. The worktree goes by its path or by the id the list shows.

What is not carried: ignored files — build outputs, node_modules, virtualenvs — are not captured and do not come back, the same trade any worktree removal makes.

## Examples

```
ff worktree                     the live worktrees, and the gone chains
ff worktree bay                 make one, on a branch of its own
ff worktree bay side            the same, on a branch that already exists
ff worktree -d bay              take one away; its work is captured first
ff worktree -d kqxmvnptul       the same, by the id the list shows
ff restore <path> --at-op <op>  bring a file back from a gone chain
```
