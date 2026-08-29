# The push boundary

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `sync`, `publish`.

Sync takes in; publish sends. Two verbs because everything sync does is undoable and a push is not. To cover:

- `ff sync`: fetch, take in what arrived, replay onto the base — in memory, landing only when clean; whose divergence it is decides what happens.
- `ff publish`: the lease (goes through only if the shared copy stands where you last saw it), and why publish deliberately does not fetch first.
- Rollback: `ff undo` then `ff publish` moves the shared copy back under a lease — not erasure, and what that means (other clones, CI, webhooks).
- The four pushes wearing one verb — create, replace, restore-deleted, roll back — and `--dry-run` as the way to ask which.
- Published history is append-only; how work lands (merge, squash, rebase) stays the team's and the forge's business.
