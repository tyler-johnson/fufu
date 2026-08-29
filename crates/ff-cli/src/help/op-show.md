One operation in full: what ran, when, on which branch, what it moved, and the diffstat of the worktree it carries against the operation before it. Bare `ff op show` reads `@`, the newest.

Every operation has a tree, which is what makes this uniform — a capture and a close are read the same way, and differ only in whether there are ref transitions to list.

-p puts the patch under the diffstat rather than in place of it: the same unified diff `ff diff` prints, for the operation instead of the tree.

## Examples

```
ff op show                     the newest operation
ff op show @^                  the one before it
ff op show kqzm                by id
ff op show -p @                what it changed, with content
ff op show --json              the same, for machines
```
