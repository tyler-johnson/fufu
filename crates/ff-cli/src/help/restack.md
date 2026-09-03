Replays a branch's commits onto the base it sits on — the branch it was forked from when one was recorded, trunk otherwise. `--onto` records a new parent first, which is how a branch is re-aimed and the only way to change it. A base is a branch wherever it lives: `origin/main` names one that lives on a remote, and re-aiming at it records it like any other.

The replay carries the branch's own commits and no others: the range stops where the branch forked from the base's history, read from the base's reflog, so a base rewritten beneath it does not hand its old commits back as the branch's. The positional names the branch being moved, so a branch you are not standing on restacks without touching a file on disk. A replay that would conflict stops with nothing changed rather than leaving you mid-rebase. A branch inside the replayed range with no commits of its own is left where it stood, and named.

The branches stacked above follow. Every local branch whose base is the one that moved is replayed onto its new tip, parent before child, through the whole tree, and the whole cascade rides the one operation, so one `ff undo` takes all of it back. A branch above whose replay conflicts is held the way the branch itself would be, with everything above it left alone; `ff switch` to it and `ff resolve` picks the replay up. A branch checked out in another worktree is skipped and the worktree named, with everything above it left alone; so is one already holding a rewrite, and one whose commits hold a merge. The output says what followed, what held, and what was skipped, and `--json` carries it as `cascade`. The exit is 3 when a branch above held, since the stack is not yet lined up.

Offline — it never reaches the network.

## Examples

```
ff restack                     replay onto the base this branch sits on
ff restack feature             restack a branch you are not standing on
ff restack --onto release-1.2  re-aim this branch and replay onto it
ff restack --onto origin/main  a base that lives on a remote
```
