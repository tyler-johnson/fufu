Replays a branch's commits onto the base it sits on — the branch it was forked from when one was recorded, trunk otherwise. `--onto` records a new parent first, which is how a branch is re-aimed and the only way to change it. A base is a branch wherever it lives: `origin/main` names one that lives on a remote, and re-aiming at it records it like any other.

The positional names the branch being moved, so a branch you are not standing on restacks without touching a file on disk. Branches inside the replayed range come along with it, and a replay that would conflict stops with nothing changed rather than leaving you mid-rebase.

Offline — it never reaches the network.

## Examples

```
ff restack                     replay onto the base this branch sits on
ff restack feature             restack a branch you are not standing on
ff restack --onto release-1.2  re-aim this branch and replay onto it
ff restack --onto origin/main  a base that lives on a remote
```
