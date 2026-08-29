Line this branch up with both things it answers to: the base beneath it and the remote copy of itself. Fetch, take in whatever arrived, replay onto the base. One verb for both, because reconciling with either is the same replay.

Nothing leaves the machine. Everything sync does is recorded and undoable, which is the whole reason it stops here — `ff publish` is the outgoing half, and it is a verb you type on purpose because a push cannot be taken back. Sync names what is waiting and leaves it.

Whose divergence it is decides what happens. Divergence this run's fetch created is somebody else's, and your commits replay on top of theirs. Divergence that was already there is yours only if fufu's own operation log accounts for every commit of it — as a rewrite it recorded, or as one it dropped as empty — and then there is nothing to take in and ff publish is what sends it. Commits the log does not recognize are somebody else's however they arrived, and they replay too.

Either replay can conflict. The first one that does stops the run and holds: nothing moves, and ff resolve picks it up.

Sync acts on the branch you are standing on. ff restack takes the name of one you are not, and cascading up a stack is one branch at a time.

## Examples

```
ff sync                        fetch, reconcile with base and remote
ff sync --no-fetch             reconcile with what you already have
ff publish                     send it, once it lines up
```
