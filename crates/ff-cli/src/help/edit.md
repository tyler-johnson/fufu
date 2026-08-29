Opens an editing session on a commit: a branch is minted at the commit and you switch to it, so the commit's real content is what gets edited, with your whole toolchain pointed at it.

The branch you came from stays exactly where it stands, its commits waiting ahead. `ff done` amends the commit with what you changed and replays them onto it. A branch name is a switch instead — the one available reading is taken and announced. Your open change parks where you stood and comes back when the session ends.

## Examples

```
ff edit 3f2a1b                 open a session on that commit
ff edit HEAD                   edit the commit you are sitting on
ff edit main                   a branch is a switch, not a session
```
