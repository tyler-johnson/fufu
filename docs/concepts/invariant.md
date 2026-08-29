# The invariant

!!! note "Draft stub"
    Planned content, not yet written. Source: DESIGN.md § The invariant.

**At every instant, the repository is a boring git repository.** To cover:

- HEAD attached to a branch, ordinary commits, `git status` legible — collaborators, CI, IDEs see nothing unusual, ever.
- The corollary: deleting fufu loses convenience, never data or comprehension.
- The strong form: abandonable and returnable at any moment — fufu's state is a cache over git, never an authority, and when they disagree the repository wins.
- Reconciliation on return is loud about anything fufu remembered that reality no longer matches.
- Compatibility, not neutrality: fufu has opinions, and they stop at the push boundary.
