# Held rewrites

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § The conflict model: held rewrites.

fufu's answer to jj's conflicted commits, without ever writing a state plain git cannot read. To cover:

- What a hold is: a rewrite that would conflict stops with nothing changed, and the intent is held rather than half-applied.
- Why: the invariant forbids `.jjconflict-*`-style trees; a conflict is a pending decision, not a commit.
- `ff resolve`: picking a held rewrite up, finishing it, or dropping it.
- What a hold blocks (publish) and what it doesn't.
- How holds appear in `ff status` and the map.
