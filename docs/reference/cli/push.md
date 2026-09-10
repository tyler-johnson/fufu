# ff push

Send a branch to its remote — the one thing fufu does that no operation log can take back. [`ff pull`](pull.md) takes in; this sends. `ff publish` is an alias: fufu's older word for this verb, kept for the fingers that learned it.

The push carries a lease: it goes through only if the shared copy still stands where you last saw it. If somebody pushed since, nothing is sent and nothing is lost — take their work in first, then send. This verb does not fetch on its own, since a lease is only worth something as the tip you last looked at.

A branch with no shared copy yet gets one, tracking set up in the same step. An upstream under another branch's name — the shape `git checkout -b feature --track origin/main` leaves — is read as the branch's base, not its copy: the push creates `origin/feature` and records `origin/main` as what the branch sits on. One that was deleted is put back under a lease that says it must not exist, which is a different act from creating one that never existed — fufu tells them apart from its record of what it has sent.

A held rewrite blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under.

- `--to <remote>` names where to send a branch that does not answer to one yet, and records the answer, so the next `ff pull` and [`ff status`](status.md) need no flag. It is refused for a branch that already answers somewhere else: one branch, one shared copy. With a single remote, or one named origin, you never need it.
- `--dry-run` says which push this would be without making it: creating a shared copy, replacing one, putting back one that was deleted, and rolling one back are four different acts wearing one verb. It writes nothing and sends nothing.

## Which branches

Bare, this is the branch you stand on. Names take one or more branches instead, from wherever you stand, and a name resolves the way [`ff restack`](restack.md) resolves one, an unambiguous prefix included; a name no branch answers to is refused before anything reaches the wire. Each branch in the run goes out under its own lease, since each has its own shared copy, and nothing comes along with it: not the bases beneath it, which `ff pull` brings in because a branch lines up with its base through them, and not what is stacked above. Name every branch of a stack to send the stack.

There is no `--all`. A push is the one act undo cannot take back, so every branch that leaves the machine is one you named or one you stand on.

## The report

The branch you stand on comes first, as it always has, then one block per other branch in the run: its name on a line of its own, and under it which push it was, or why nothing went. A run that sent anything closes with the two lines about what left the machine. When names left the branch you stand on out of the run, nothing is said of it.

A run of one branch is the verb as it always was: a lease the remote refuses is the run's error, and the exit is 1. Among several, a refusal is that branch's alone. The rest of the run still goes out, the refused branch's block says what the wire said and the way out, and the exit is 1 once the run is over. A hold blocks that branch and no other, and the exit is 3 when one did.

With `--json`, `branches` is every branch in the run, one row each with its `push`, whether it was `pushed`, and the `error` the wire answered with or null; `push` and `pushed` are the branch you stand on, and read `NotNamed` and false when the run did not reach it.

## Taking a push back

The way back is this verb rather than ff undo: undo the commit and push again, and the lease rolls the shared copy back to where the branch now stands. That is not erasure — other clones may hold the commits, CI ran, a webhook fired — but the shared copy is yours to move.

## Usage

```
Usage: ff push [OPTIONS] [branch]...

Arguments:
  [branch]...
          Branches to push, each under its own lease; without any, the one you are on

Options:
  -n, --dry-run
          Say which push this would be, without sending it

      --to <remote>
          Send to this remote, and record that the branch answers to it

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

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
