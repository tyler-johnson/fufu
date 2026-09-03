# ff doctor

A safety net you cannot inspect is not trustworthy, and every floor of this one can degrade quietly: a log moved by something that is not fufu, a reflog that never got created, the gc guard deleted out of local config, a branch that answers to no remote anything can name, hooks never installed, a stale binary. Doctor reads the whole net in one pass — the engine (the operation log and its age, the fufu identity on its tip, reflogs, the gc guard, log health and pending foreign drift, settings validated through the readers' own parsers, a trim preview and the auto-trim clock), the remote floor (whether every branch can name the remote it answers to, config left naming branches that are not here, and tracking refs that have gone), the wiring (agent hooks, the shell alias, and a warning when nothing at all feeds capture), commit signing (whether it is on, and whether the format, program and key it names will actually work), and the update lane.

Rows come at three levels: ok counts nothing, info is news rather than a problem, WARN is a finding. Findings drive the exit code — 0 healthy, 1 findings — so CI can gate on it, and --json emits the same rows for machines.

Read-only by design: doctor reports the drift the log will absorb and never absorbs it, takes no snapshot, reconciles nothing. --fix is the one consented write, and it repairs exactly two things: the gc reflog-expiry keys, and a config section left naming a branch that is gone from both sides. It never touches a section whose shared copy is still standing — that one is [`ff branch delete`](branch-delete.md) doing its job, not drift.

## Usage

```
Usage: ff doctor [OPTIONS]

Options:
      --fix
          Repair the gc config keys (the one write doctor performs)

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
ff doctor                      read the net
ff doctor --fix                repair gc keys and dead config (the only write)
ff doctor --json               the same rows, for machines
```
