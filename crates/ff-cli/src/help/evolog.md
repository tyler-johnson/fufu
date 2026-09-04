Every operation on the change you have open, newest first — the drill-in behind the letters column in `ff log`. This is where a lost hour is found: each row is a whole worktree, and `ff restore --at-op <id>` brings any of them back. `ff ev` is the short spelling.

Because fufu captures before it works, the newest row is often this command's own capture, taken a moment ago when it found the tree dirty. That is intended.

Ids are spelled in the letters k–z, never hex digits, so an operation id can never be misread as a commit sha. The bold prefix is the shortest one `ff op` and `--at-op` resolve unambiguously.

-p prints each row's patch under it — what that one operation changed, measured against the capture before it on this branch.

## Examples

```
ff evolog                      the open change's operations
ff evolog -n 0                 all of them
ff evolog -p                   each row with what it changed, in full
ff restore src/ --at-op <id>   pull a directory back from one
```
