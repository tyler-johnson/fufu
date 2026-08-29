# fufu vs jj

!!! note "Draft stub"
    Planned content, not yet written — but this is the thesis page, and DESIGN.md § Thesis is most of it. Anyone who has heard of fufu reads this first.

To cover:

- What jj got right, taken wholesale: automatic capture, no dirty state, switch at will, auto-rebasing descendants, undoable operations.
- The architectural difference: jj is a new VCS with git as a storage backend — its store is authoritative, the git repo a projection. fufu inverts it: git remains the VCS, fufu is the pilot.
- What the inversion buys: attached HEAD, branches that move as you work, no `.jjconflict-*` trees, git commands first-class, abandonable at any moment.
- What jj has that fufu deliberately doesn't: revsets over a first-class change graph, conflicted commits as mergeable objects, colocated-repo independence from git semantics.
- Held rewrites versus conflicted commits: two answers to the same problem, and what each costs.
- The name, the dojo, and the debt: fufu exists because jj proved the workflow.
