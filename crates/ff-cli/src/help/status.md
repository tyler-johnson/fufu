Where you are and what is uncommitted: the branch, its upstream, the open change, and the files that differ from the commit underneath it. `ff st` is the short spelling.

The files are a diffstat — counts, not content. `ff diff` is the same change read down to the line, and it sees the untracked files `git diff` does not.

`--json` also carries the orientation an agent asks for first: the checkout root and which worktree this is, the base the branch sits on and how far above it the branch stands, the remote the branch answers to, and the last operation on this worktree with its session — `ff op log --json`'s own row, never the read's own capture.

Status is also where drift is loud. Work done behind fufu's back — a plain `git commit`, a rebase run by a tool that never heard of fufu — is absorbed into the operation log lazily, and status keeps reporting it until the next fufu operation, so foreign motion is never silent. Status reports the motion as one line, the count and the shape of what moved, and `ff op show @` lists every ref it moved.

## Examples

```
ff status
ff status --json               the same state, for scripts
ff diff                        the same change, with content
```
