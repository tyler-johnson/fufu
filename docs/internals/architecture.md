# Architecture

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § Architecture: three floors.

The three floors, rewritten as description rather than proposal:

- Floor 1: capture — snapshots at machine rate, the operation log, absorption of foreign motion.
- Floor 2: futures — merge simulation in memory, cached by (base, ours, theirs), so `ff status` can say "rebases cleanly" before anything moves.
- Floor 3: the verbs — every operation as a transition between boring git states.
- Where fufu's state lives on disk, and why every piece of it is disposable (cache over git, never authority).
