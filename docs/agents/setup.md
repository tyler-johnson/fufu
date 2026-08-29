# Agent setup

!!! note "Draft stub"
    Planned content, not yet written. Sources: `fufu.gitPolicy` docs commit, the repository's own hook configuration.

To cover:

- `fufu.gitPolicy`: what each policy level does, and which to give an agent.
- A paste-ready CLAUDE.md / AGENTS.md block: use `ff` for writes, `ff commit -m` closes the change, `ff undo` takes back the last operation, `ff git <args>` for everything else git does.
- A paste-ready hook (Claude Code `UserPromptSubmit` or equivalent) that briefs the agent every turn — the pattern this repository itself uses.
- Skills: shipping a `fufu` skill so the agent reads recovery flows before improvising with `ff git`.
- Verifying the setup: what `ff doctor` should say, and a smoke test (have the agent break something, then `ff undo` it).
