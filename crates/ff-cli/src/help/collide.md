Would these two branches hit each other if both landed? The comparison replays a three-way merge in memory without changing branches, the index, or worktree files. The CLI can still capture, auto-fetch, and run maintenance around that comparison.

The other two axes are vertical: a branch against the base beneath it, or against the remote copy of itself. This one runs sideways, between two branches where neither sits under the other, which is the pair no other verb asks about.

Each side is judged on the tree the operation log holds for it, not the one on disk, so a branch checked out in another worktree — or nowhere at all — still answers, and uncommitted work counts. A name wears * when that is what happened.

A collision is a finding rather than a failure: the exit is 0 whichever way the answer goes, and a program reads the verdict from --json.

## Examples

```
ff collide feat-x              against the branch you are on
ff collide feat-x feat-y       two you name
ff collide feat-x --json       the verdict, for a program
```
