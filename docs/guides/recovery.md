# Recovery

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `undo`, `history`, `op`, `restore`; the `fufu` skill's recovery flows.

The cookbook for when something is already wrong. Scenarios to cover, each as symptom → command → transcript:

- "An agent ran `git reset --hard`" — one `ff undo`, foreign motion absorbed.
- "I want one file back the way it was" — `ff restore <path>`, and restoring from a named revision.
- "I want the whole tree from twenty minutes ago" — `ff history`, then `ff op restore <id>`.
- "I undid too far" — `ff redo`, and why landing new work forks the redo path instead of destroying it.
- "I committed to the wrong branch / with the wrong message" — pointers into [rewriting history](rewriting-history.md).
- "Someone force-pushed over my branch" — sync's divergence rules, and the lease that would have caught it.
- What undo cannot reach: the push, and anything before the floor.
