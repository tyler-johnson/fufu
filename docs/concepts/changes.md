# Changes

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `commit`, `describe`, `switch`, `start`.

The working tree is the change. To cover:

- The three states: open (the tree, being edited), parked (left with a branch on switch), closed (a commit).
- No staging area: closing is the commit, and path arguments to `ff commit` close a slice — selection at the moment of the close, not a state you maintain.
- Pending descriptions: `ff describe` names work before it is a commit; `ff commit` picks the name up.
- Parking: what travels with a branch on `ff switch` (files, edits, pending description) and what never crosses a fork (`ff start` opens clean).
- The `@` row in `ff`, `ff log`, and `ff status`: one notation for the open change everywhere.
