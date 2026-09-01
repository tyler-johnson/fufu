# fufu vs jj

fufu exists because jj proved the workflow. This page is the argument for why fufu is not jj, and it is the honest place to start if you have used either tool.

## What jj got right, taken wholesale

jj demonstrates a better daily workflow than git's, and fufu takes it without argument:

- All work is automatically saved, always. There is no dirty state, no stash, no lost file.
- You can switch between lines of work at will and everything is simply ready.
- Editing any commit in a stack automatically rebases its descendants.
- Every operation is undoable.

If you already live in jj, nothing in that list will feel new in fufu. The disagreement is entirely about what those properties cost.

## The architectural difference

jj achieves its workflow by being a **new VCS that treats git as a storage backend**. Its own store is authoritative; the git-visible repository is a projection of it. That one decision is the source of everything uncomfortable about jj for a git-fluent user: detached HEADs, branches that don't move as you work, commits with machine-generated `.jjconflict-*` trees, git commands demoted to second class.

None of the workflow benefits require that decision. fufu inverts it:

> **git remains the VCS. fufu is the pilot.**

fufu is a daily interface layered on an ordinary git repository. It owns the ephemeral and the automatic — capture, movement, history rewriting, undo — and leaves the durable graph entirely to git. The consequence is [the invariant](../concepts/invariant.md): at every instant, the repository is a boring git repository, and fufu's own state is a cache over it, never an authority.

## Side by side

| | jj | fufu |
| --- | --- | --- |
| authority | jj's store; the git repo is a projection | git; fufu's state is a disposable cache |
| HEAD | detached, by design | attached to a branch, always |
| branches | bookmarks that sit still until you move them | real refs that move as you work; anonymous ones are still refs from birth |
| a conflict | an object in the graph — a commit holding a merge expression | an operation held pending — the *absence* of the new commit |
| your other tools | see the projection, plus states plain git can't comprehend | see an ordinary git repository, always |
| raw git commands | legal in a colocated repo, then imported: the motion is settled once jj re-reads the git refs at its next command | first class: absorbed into the operation log, loudly, and undoable |
| leaving | colocated: delete `.jj` and the commits and bookmarks stay; the op log, the change ids, and any unresolved conflict go with it | walk away any moment; return and reconcile |

## What the inversion buys

**Legibility.** A colocated jj repo keeps the git-visible picture close — commits land in the git object store as jj makes them, and bookmarks export to git branches — but a seam remains, with its own etiquette: a detached git HEAD as the normal state, anonymous working-copy commits a GUI shows without explanation, and the rule that motion made with raw git is only settled once jj has imported it. fufu has no seam to keep settled: there is one store, so collaborators, CI, IDEs, GUIs, and every plain-git tool see an attached HEAD and ordinary branches, always.

**Abandonability.** Deleting fufu loses convenience, never data or comprehension. The strong form: fufu is abandonable and returnable at any moment — a GUI session, a teammate's raw git, a weekend on a machine without fufu are all legitimate, all absorbed. When fufu's records disagree with the repository, the repository wins and fufu rebuilds its picture, loudly. Colocation narrows jj's version of this gap without closing it: the commits and exported bookmarks are already in the git repository, so walking away strands no branch — but the op log and undo, the change ids, and any unresolved conflict live in `.jj` and nowhere else, so deleting `.jj` is accepting those losses, and a weekend of raw git is a desync to import on return rather than a shrug.

**git fluency keeps paying.** Your reflexes, your team's review habits, and twenty years of git tooling all still apply. fufu asks for a workflow shift — rebase-onto-main, malleable unpublished commits, leased force-pushes as routine — but never a translation layer over what a repository *is*.

## What fufu gives up

The inversion has real costs, and DESIGN.md names them rather than hiding them:

- **You cannot build on a post-rewrite state before its conflicts are resolved.** jj lets you keep stacking on top of a conflicted commit; in fufu the rewrite is held, and you keep working at the existing tip while the pending rewrite replays over whatever you add.
- **Conflicted commits cannot be shipped around.** In jj a conflict is an object you could in principle push; fufu never creates one — which you would never want to push anyway, but the capability is genuinely absent.
- **jj's conflicts simplify as they propagate**, because they are expressions; fufu carries conflicts forward as literal marker text, and text does not simplify itself. A later commit can leave a mark alone, not dissolve it.
- **The operation log has a floor.** jj is present from a repository's birth in a way an adopted overlay is not: `ff undo` reaches back to the moment the repository was armed, and no further.

## Two answers to conflicts

This is the deepest divergence, and it deserves its own sentence-length summary: jj stores an unresolved conflict *inside* the resulting commit; fufu stores it as the *absence* of the resulting commit.

fufu's observation is that for a human, conflicts are operation-shaped, not edit-shaped — and the user-visible benefit of jj's model is scheduling: the conflict doesn't interrupt you, and you resolve it when you choose. That benefit survives translation. When a rewrite would conflict, nothing is touched; the intent is held, both inputs stay ordinary git commits, and `ff resolve` materializes the whole thing on your schedule — every standing region in one editing session, each side labeled with the step that wrote it, and the entire rebased stack landing at once when you finish. No conflicted state ever exists in the graph.

Deferral only works because jj paired it with relentless disclosure, and held rewrites inherit all three of its disciplines: announced at creation, pinned in every status until gone, and blocking the exit — `ff publish` refuses a branch with a held rewrite, the way jj refuses to push conflicted commits. [Held rewrites](../concepts/held-rewrites.md) has the full model.

## Choosing

Choose jj if you want conflicts as first-class mergeable objects, you are happy to let a new VCS be the authority, and the people and tools around you can live with the projection. It is the more radical design, executed with taste, and fufu's debt to it is total.

Choose fufu if the repository must stay legible to every git tool and teammate at every instant, if you want the option of walking away without an export, or if agents work in your repository and you want their raw git commands captured and undoable rather than fenced off.

## The name

jj is short for Jujutsu, the martial art of redirecting force instead of opposing it. fufu answers from the same dojo — "fu" is the syllable hacker culture borrowed for tool mastery, doubled. The binary is `ff`: the left hand's mirror of `jj`, a double-tap on the index finger's home key. The lineage is on purpose; so is the inversion.
