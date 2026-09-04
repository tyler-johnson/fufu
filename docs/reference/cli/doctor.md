# ff doctor

Reads fufu's whole safety net in one pass and reports what it finds. Read-only by design: it takes no snapshot, reconciles nothing, and reports the drift the log will absorb rather than absorbing it.

What it reads, in order:

- The engine — the operation log and its age, the fufu identity on its tip, reflogs, the gc guard, log health and pending foreign drift, settings validated through the readers' own parsers, a trim preview and the auto-trim clock.
- The remote floor — whether every branch can name the remote it answers to, config left naming branches that are not here, and tracking refs that have gone.
- The wiring — agent hooks, the shell alias, and a warning when nothing at all feeds capture.
- Extensions — every `ff-<name>` found on PATH, whether it is declared, whether a declared one still matches the version and contract recorded at [`ff extension add`](extension-add.md), and whether one that promised tools produced any.
- Commit signing — whether it is on, and whether the format, program and key it names will actually work — then the update lane.

Rows come at three levels: ok counts nothing, info is news rather than a problem, WARN is a finding. Findings drive the exit code — 0 healthy, 1 findings — so CI can gate on it, and --json emits the same rows for machines.

## The one write: --fix

It repairs exactly two things: the gc reflog-expiry keys, and a config section left naming a branch that is gone from both sides. It never touches a section whose shared copy is still standing — that one is [`ff branch delete`](branch-delete.md) doing its job, not drift.

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
