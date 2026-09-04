Send this branch to its remote. The outgoing half of lining up, and the one thing fufu does that no operation log can take back — which is exactly why it is a verb you type rather than a default riding along inside another one. `ff sync` takes in; this sends.

There is a way back, and it is this verb rather than ff undo: undo the commit and publish again, and the lease rolls the shared copy back to where the branch now stands. That is not erasure — other clones may hold the commits, CI ran, a webhook fired — but the shared copy is yours to move, and fufu records every push so it knows which commits out there are your own.

The push carries a lease: it goes through only if the shared copy still stands where you last saw it. If somebody pushed since, nothing is sent and nothing is lost — ff sync takes their work in first, and this sends afterwards.

A branch with no shared copy yet gets one, tracking set up in the same step. One that was deleted is put back under a lease that says it must not exist; one that was never created is simply created, and telling those two apart is why fufu keeps a record of what it has sent.

Publish does not fetch, on purpose. The lease is worth something precisely because it means the tip you last looked at; refreshing it first would ask git to guard you against a change you accepted without reading.

A held rewrite blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under.

`--to <remote>` names where to send a branch that does not answer to one yet, and records the answer, so the next ff sync and ff status need no flag. It is refused for a branch that already answers somewhere else: one branch, one shared copy. With a single remote, or one named origin, you never need it.

--dry-run says which push this would be without making it: creating a shared copy, replacing one, putting back one that was deleted, and rolling one back are four different acts wearing one verb, and this is the only way to tell them apart while the answer still costs nothing. It writes nothing and sends nothing.

## Examples

```
ff publish                     send this branch, under a lease
ff publish -n                  which push would this be? send nothing
ff publish --to upstream       send to a named remote, and remember it
ff sync                        take in what arrived, first
ff status                      what is waiting to go, before you send it
```
