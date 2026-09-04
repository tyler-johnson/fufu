# ff publish

Send this branch to its remote — the one thing fufu does that no operation log can take back. [`ff sync`](sync.md) takes in; this sends.

The push carries a lease: it goes through only if the shared copy still stands where you last saw it. If somebody pushed since, nothing is sent and nothing is lost — sync takes their work in first, and this sends afterwards. Publish does not fetch on its own, since a lease is only worth something as the tip you last looked at.

A branch with no shared copy yet gets one, tracking set up in the same step. One that was deleted is put back under a lease that says it must not exist, which is a different act from creating one that never existed — fufu tells them apart from its record of what it has sent.

A held rewrite blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under.

- `--to <remote>` names where to send a branch that does not answer to one yet, and records the answer, so the next sync and [`ff status`](status.md) need no flag. It is refused for a branch that already answers somewhere else: one branch, one shared copy. With a single remote, or one named origin, you never need it.
- `--dry-run` says which push this would be without making it: creating a shared copy, replacing one, putting back one that was deleted, and rolling one back are four different acts wearing one verb. It writes nothing and sends nothing.

## Taking a publish back

The way back is this verb rather than ff undo: undo the commit and publish again, and the lease rolls the shared copy back to where the branch now stands. That is not erasure — other clones may hold the commits, CI ran, a webhook fired — but the shared copy is yours to move.

## Usage

```
Usage: ff publish [OPTIONS]

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
ff publish                     send this branch, under a lease
ff publish -n                  which push would this be? send nothing
ff publish --to upstream       send to a named remote, and remember it
ff sync                        take in what arrived, first
ff status                      what is waiting to go, before you send it
```
