# Snapshots and undo

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `undo`, `redo`, `history`, `op`; DESIGN.md § Architecture.

To cover:

- Capture: the tree is snapshotted before every action, at machine rate; captures outnumber verb operations by an order of magnitude.
- The operation log: every mutation fufu made, plus foreign ones absorbed lazily; `ff op log` and its address space.
- Undo steps over *runs*, not operations: a stretch of adjacent captures is one keystroke, a verb is always its own step — which keeps undo from rolling past a commit by accident.
- Undo moves the log's pointer rather than appending, so the log records work, never navigation; `ff redo` comes forward, and landing new work forks the log rather than truncating it.
- `ff history` as the keystroke map: rows below `@` are undos, rows above are redos, and every row is an `ff op restore` target.
- The floor: `operation log initialized from observed state; earlier operations not undoable`.
