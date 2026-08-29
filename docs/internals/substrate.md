# Substrate

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § Substrate.

To cover:

- Rust on gitoxide; git defines the semantics, fufu chooses the execution per call-site.
- Reads native from day one; writes climb a ladder as trust grows; the wire is climbed except for sending (the push stays spawned until gix can send a pack).
- Differential testing against the git binary as the permanent compatibility contract.
- Behavioral compatibility: hooks run, hook-runners see a correct index at the right moment.
- No daemon; the git-free destination and the honest staging toward it.
