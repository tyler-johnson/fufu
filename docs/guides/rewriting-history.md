# Rewriting history

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `describe`, `absorb`, `edit`, `done`, `lift`, `restack`, `collide`, `trim`; the `fufu` skill.

Everything you'd reach for `rebase -i` for, without the todo list. Restacking above the target is automatic in every verb here. To cover:

- Reword a closed commit: `ff describe <rev> -m`.
- Fold the open change into a closed commit: `ff absorb --into <rev>`, with path filters for a partial fold.
- Reopen a closed commit: `ff edit <rev>`, then `ff done` (one operation: amend, replay, return) or `ff done --abandon`.
- Split: closing slices with `ff commit <paths>`, and splitting commits that already closed.
- `ff lift`, `ff collide`, `ff trim`: what each is for, with before/after maps.
- The boundary: published commits are append-only; what fufu refuses to rewrite and why.
