# ff doctor

Checks fufu's operation log, configuration, and installed integrations. The CLI attempts a capture before checking, which can initialize the log and absorb foreign ref changes. It also fetches before every check when automatic fetching is enabled (`--no-fetch` skips it; CI skips it unless `--fetch` is explicit). Automatic trimming and update maintenance can run after the report, including when it contains findings. The diagnostic checks themselves report findings; `--fix` enables their repairs.

What it reads, in order:

- The engine — the operation log and its age, the fufu identity on its tip, reflogs, the gc guard, log health and pending foreign drift, settings validated through the readers' own parsers, a trim preview, the auto-trim clock, and the auto-fetch clock — when the tracking refs were last refreshed, or since when the remote has not answered.
- The remote floor — whether every branch can name the remote it answers to, config left naming branches that are not here, and tracking refs that have gone.
- The wiring — agent hooks, the shell alias, and a warning when nothing at all feeds capture.
- Extensions — every `ff-<name>` found on PATH.
- Commit signing — whether it is on, and whether the format, program and key it names will actually work — then the update lane.

Rows come at three levels: ok counts nothing, info is news rather than a problem, WARN is a finding. Findings drive the exit code — 0 healthy, 1 findings — so CI can gate on it, and --json emits the same rows for machines.

## Repairs: --fix

It repairs the gc reflog-expiry keys, config sections naming branches gone from both sides, partial or stale managed hook installations, and stale shipped skills. It never removes a branch config section whose shared copy is still standing — that one is [`ff branch -d`](branch.md) doing its job, not drift.

## Usage

```
Usage: ff doctor [OPTIONS]

Options:
      --fix
          Repair the gc config keys (the one write doctor performs)

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

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
ff doctor --fix                repair the findings marked fixable
ff doctor --json               the same rows, for machines
```
