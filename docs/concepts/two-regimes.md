# The two regimes

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § The two regimes.

fufu's guarantees follow its surface: inside it, jj's rules apply; outside it, git's rules apply — exactly. To cover:

- What "inside" buys: automatic capture, undoable operations, futures in `ff status`.
- What "outside" means: any git command, GUI, IDE, or teammate acting on the same repository.
- Lazy absorption: foreign motion is noticed at the next fufu operation, reported loudly in `ff status`, and folded into the operation log so `ff undo` can reach it.
- A weekend without fufu: returning is reconciliation, not recovery.
