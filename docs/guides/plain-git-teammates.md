# Plain-git teammates

!!! note "Draft stub"
    Planned content, not yet written. Sources: DESIGN.md § The invariant, § The two regimes.

Nobody else has to know you run fufu. To cover:

- What a teammate, a GUI, an IDE, or CI sees: an ordinary git repository, always.
- Using git tools yourself alongside fufu: reads are always fine; writes are absorbed lazily and loudly.
- `ff git <args>`: snapshot first, then run git verbatim — the escape hatch that keeps undo working.
- The alias and strict mode: what `git` typed inside a fufu repo does, and what strict refuses.
- A weekend on a machine without fufu: leave, work, return, reconcile.
- What fufu asks of the *branch* (rebase-onto-main, leased force-pushes) versus what it asks of the *repo* (nothing).
