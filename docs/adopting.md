# Adopting fufu

!!! note "Draft stub"
    Planned content, not yet written.

`ff init` in a repository git made means *turn fufu on here*. To cover:

- What arming does: the gc guard, the operation log's floor, and what `ff undo` can reach from day one (nothing before the floor).
- What does not change: refs, history, remotes, hooks, CI, teammates — the [invariant](concepts/invariant.md) in practice.
- The workflow shift you are accepting: rebase-onto-main, malleable unpublished commits, leased force-pushes as routine (from DESIGN.md's thesis).
- Trying it and leaving: fufu is abandonable and returnable; deleting it loses convenience, never data.
- Adopting mid-flight: what happens to a dirty tree, an in-progress rebase, existing stashes.
