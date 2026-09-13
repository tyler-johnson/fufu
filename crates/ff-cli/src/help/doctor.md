Check fufu's operation log, configuration, and installed integrations. By default, report findings; `--fix` repairs supported problems. The CLI also attempts capture and can fetch and run maintenance, even without `--fix`.

## Examples

```sh
ff doctor                       # Check this repository and machine setup
ff doctor --no-fetch            # Check using existing tracking refs
ff doctor --fix                 # Repair supported findings
ff doctor --json                # Read diagnostic rows in a script
```

### Options

### Findings and repairs

Rows are `ok`, `info`, or `WARN`. Warnings count as findings: exit 0 means healthy, exit 1 means findings. JSON contains the same rows.

`--fix` repairs garbage-collection reflog-expiry keys, configuration for branches gone from both local and remote sides, partial or stale managed hooks, and stale shipped skills. It does not remove branch configuration while a remote copy still exists.

### Capture, fetch, and maintenance

Before checking, the CLI attempts a snapshot, which can initialize the operation log and reconcile outside ref changes. It fetches every run when automatic fetching is enabled. `--no-fetch` skips that; CI skips it unless `--fetch` is explicit. Automatic trimming and update maintenance can run after the report, including a report with findings.

### Checks performed

- Operation history, identity, reflogs, garbage-collection protection, pending outside changes, settings, retention preview, and automatic trim/fetch timing.
- Branch remote assignments, stale branch configuration, and deleted tracking refs.
- Agent and shell integrations, including a warning when no hooks feed capture.
- Executable extensions named `ff-<name>` on PATH.
- Commit signing format, program, and key, followed by update configuration.
