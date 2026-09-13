Send a branch to its remote — the one thing fufu does that no operation log can take back. `ff pull` takes in; this sends. `ff publish` is an alias: fufu's older word for this verb, kept for the fingers that learned it.

The push carries a lease: the remote ref must still match the expected tip when Git updates it. Replacing commits requires that the tracking ref match fufu's seen record, written by `ff pull`, `ff switch` onto a remote's branch, `ff clone`, and successful pushes. A fast-forward can proceed without that agreement because it removes no commits. A fetch by an editor, `ff git fetch`, or `ff pull --dry-run` updates tracking refs without updating the seen record. Push itself can auto-fetch on `fufu.autoFetch`'s cadence before planning; `--no-fetch` skips it. That fetch does not authorize replacing an unseen remote tip, and the wire lease catches later races.

There is no branch-ownership check or special protection for `main`. Local rewrite verbs accept already-pushed commits, and push can send those rewrites when its guards pass. Team policy and server-side branch protection decide whether shared history may be rewritten.

A branch with no shared copy yet gets one, tracking set up in the same step. An upstream under another branch's name — the shape `git checkout -b feature --track origin/main` leaves — is read as the branch's base, not its copy: the push creates `origin/feature` and records `origin/main` as what the branch sits on. One that was deleted is put back under a lease that says it must not exist, which is a different act from creating one that never existed — fufu tells them apart from its record of what it has sent.

A held rewrite blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under.

- `--to <remote>` names where to send a branch that does not answer to one yet, and records the answer, so the next `ff pull` and `ff status` need no flag. It is refused for a branch that already answers somewhere else: one branch, one shared copy. With a single remote, or one named origin, you never need it.
- `--dry-run` says which push this would be without sending a remote update or moving local branches and files. Automatic fetching and maintenance can still run; the push's own capture and push record are skipped.

### Which branches

Bare, this is the branch you stand on. Names take one or more branches instead, from wherever you stand, and a name resolves the way `ff restack` resolves one, an unambiguous prefix included; a name no branch answers to is refused before anything reaches the wire. Each branch in the run goes out under its own lease, since each has its own shared copy, and nothing comes along with it: not the bases beneath it, which `ff pull` brings in because a branch lines up with its base through them, and not what is stacked above. Name every branch of a stack to send the stack.

There is no `--all`. A push is the one act undo cannot take back, so every branch that leaves the machine is one you named or one you stand on.

### The report

The branch you stand on comes first, as it always has, then one block per other branch in the run: its name on a line of its own, and under it which push it was, or why nothing went. A run that sent anything closes with the two lines about what left the machine. When names left the branch you stand on out of the run, nothing is said of it.

A run of one branch is the verb as it always was: a lease the remote refuses is the run's error, and the exit is 1. Among several, a refusal is that branch's alone. The rest of the run still goes out, the refused branch's block says what the wire said and the way out, and the exit is 1 once the run is over. A hold blocks that branch and no other; the exit is 3 for a blocked branch unless a refusal makes it 1.

With `--json`, `branches` is every branch in the run, one row each with its `push`, whether it was `pushed`, and the `error` the wire answered with or null; `push` and `pushed` are the branch you stand on, and read `NotNamed` and false when the run did not reach it.

### Taking a push back

To roll back a push, undo the commit locally and push again, under the same lease and server-side rules. That moves the remote branch back; it does not erase commits from other clones, CI runs, or webhooks that already fired.

## Examples

```
ff push                        send this branch, under a lease
ff push side                   send side, from wherever you stand
ff push a b                    two branches, each under its own lease
ff push -n                     which push would this be? send nothing
ff push --to upstream          send to a named remote, and remember it
ff pull                        take in what arrived, first
ff status                      what is waiting to go, before you send it
```
