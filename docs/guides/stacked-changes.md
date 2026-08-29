# Stacked changes

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `restack`, `sync`, `absorb`.

Working on a branch whose base is itself a branch under review. To cover:

- Starting a stack: `ff start <branch>` forks at a branch's tip.
- Review feedback lands with `ff absorb` and the stack above restacks itself.
- `ff sync` acts on the branch you stand on; `ff restack <branch>` reaches one you don't; cascading up a stack is one branch at a time.
- Publishing a stack: each branch under its own lease.
- Reading a stack in the map.
