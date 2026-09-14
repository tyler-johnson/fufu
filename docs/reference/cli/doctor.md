# ff doctor

Check fufu's operation log, configuration, and installed integrations. By default, report findings; `--fix` repairs supported problems. The CLI also attempts capture and can fetch and run maintenance, even without `--fix`.

## Usage

```
Usage: ff doctor [OPTIONS]
```

## Examples

```sh
ff doctor                       # Check this repository and machine setup
ff doctor --no-fetch            # Check using existing tracking refs
ff doctor --fix                 # Repair supported findings
ff doctor --json                # Read diagnostic rows in a script
```

## Options

```
Options:
      --fix
          Repair supported config, managed-hook, and shipped-skill findings

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Findings and repairs

Rows are `ok`, `info`, or `WARN`. Warnings count as findings: exit 0 means healthy, exit 1 means findings. JSON contains the same rows.

`--fix` repairs garbage-collection reflog-expiry keys, configuration for absent local branches with no surviving tracking ref, partial or stale managed hooks, and stale shipped skills. A surviving tracking ref keeps its branch configuration.

Hook checks inspect installed files, not a running client or its trust approval. Restart or reload after repairs; review changed Codex hooks through `/hooks`. With `--no-fetch`, branch checks use cached tracking refs.

## Capture, fetch, and maintenance

Before checking, the CLI attempts a snapshot, which can initialize the operation log and reconcile outside ref changes. It fetches every run when automatic fetching is enabled. `--no-fetch` skips that; CI skips it unless `--fetch` is explicit. Automatic trimming and update maintenance can run after the report, including a report with findings.

## Checks performed

- Operation history, identity, reflogs, garbage-collection protection, pending outside changes, settings, retention preview, and automatic trim/fetch timing.
- Branch remote assignments, stale branch configuration, and deleted tracking refs.
- Agent and shell integrations, including a warning when no hooks feed capture.
- Executable extensions named `ff-<name>` on PATH.
- Commit signing format, program, and key, followed by update configuration.
