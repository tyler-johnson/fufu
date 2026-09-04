The branch's pointer into the log moves to trash rather than evaporating, its parked change — the open work it was holding — is demoted to an ordinary stash entry, and the tip stays pinned by the operation. Nothing local is lost, there is no merged-check to argue with, and `ff undo` brings the branch and its timeline back.

The branch's operations themselves stay on the log either way; what goes is the way in through this name.

### A published branch

There is more than the name: a copy on the remote, and a tracking ref and upstream pointing at it. A plain delete leaves all three standing and says so.

`--shared` deletes the copy too, under a lease — only if it still stands where you last saw it — and takes the tracking ref and upstream down with it. That half left the machine: the branch still comes back, and the copy does not.

## Examples

```
ff branch delete old-experiment
ff branch delete ff/misty-owl    an anonymous one you are done with
ff branch delete spike --shared  the copy on the remote goes too
ff undo                          put the branch back
```
