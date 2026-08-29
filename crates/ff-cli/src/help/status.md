Where you are and what is uncommitted: the branch, its upstream, the open change, and the files that differ from the commit underneath it.

The files are a diffstat — counts, not content. `ff diff` is the same change read down to the line, and it sees the untracked files `git diff` does not.

Status is also where drift is loud. Work done behind fufu's back — a plain `git commit`, a rebase run by a tool that never heard of fufu — is absorbed into the operation log lazily, and status keeps reporting it until the next fufu operation, so foreign motion is never silent.

## Examples

```
ff status
ff status --json               the same state, for scripts
ff diff                        the same change, with content
```
