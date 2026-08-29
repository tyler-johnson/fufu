# The machine surface

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § The machine surface; per-verb `--json` help.

To cover:

- `--json` on the readers: `ff status --json`, `ff history --json`, `ff log --json` — schemas, with examples.
- Output contracts: one line per fact, stable field names, what is promised across versions and what is not.
- Exit codes, and what strict mode's exit 2 means.
- Piped output never pages; how to detect fufu programmatically (`ff root`, `ff version`).
- Reading the operation log from a script: `ff op log --json` and addressing operations by id.
