# The invariant

**At every instant, the repository is a boring git repository.**

HEAD is attached to a branch. Commits are ordinary commits. `git status` reads the way it always reads. Collaborators, CI, IDEs, and plain-git tooling see nothing unusual, ever. fufu never creates a state that plain git cannot represent; it only automates the transitions between such states.

This is fufu's one non-negotiable promise, and every other design question in the tool is settled by asking what preserves it. It shows up in the mechanisms: [snapshots](snapshots-and-undo.md) live in refs outside the visible graph, so the commit history you and your teammates read is untouched; a [parked change](branches.md) is an ordinary stash entry labeled with its branch, sitting in the same stash panel every GUI already has. Where jj gets its workflow by making its own store authoritative and projecting a git repository from it — which is where detached HEADs and machine-generated conflict commits come from — fufu keeps git authoritative and confines itself to states git already understands. That difference is the thesis of [fufu vs jj](../comparisons/vs-jj.md).

## Deleting fufu loses convenience, never data

The first corollary: because everything fufu writes is ordinary git, removing fufu costs you the automation and nothing else. Every commit, every branch, every snapshot ref, every parked stash entry is still there, still legible, still reachable with plain git commands. A user stripped of fufu is slower, not stranded — the stash dance comes back, the manual rebase comes back, but no work and no understanding is lost.

Comprehension matters as much as data here. A repository fufu has been driving does not need fufu to be explained. A teammate who opens it in a GUI sees branches where branches should be and a stash entry named `fufu: wip on feature-x` next to the button that restores it. Nothing requires the reader to know fufu exists.

## A cache over git, never an authority

The corollary has a strong form: fufu is abandonable and returnable at any moment, not merely removable once. A GUI session, a teammate's raw git, a weekend on a machine without fufu — all legitimate, all absorbable.

Supporting that forces one deep design rule. fufu's own state — the operation log, the rewrite map, parked changes, [held rewrites](held-rewrites.md) — is a cache over git, never an authority. When fufu's records disagree with what the repository actually contains, the repository wins, and fufu rebuilds its picture from what it observes. There is no state file that must be kept consistent with the repository for the repository to be valid; the repository is valid on its own, and fufu's records are a derived convenience.

## Reconciliation is loud

Returning to a repository after working around fufu is reconciliation, not recovery. Nothing is broken and nothing needs repair; fufu compares what it remembered against what it finds, absorbs the foreign operations into its timeline, and carries on. But reconciliation is loud: anything fufu remembered that reality no longer matches gets said out loud, not silently forgotten. A branch that moved, a parked entry that was dropped, a commit that was rewritten behind fufu's back — each is reported so you know what changed while fufu was not watching.

This is one half of a broader boundary. Operations that go through fufu get fufu's guarantees; operations that go around it get git's exact documented behavior, captured and absorbed afterward. [The two regimes](two-regimes.md) covers that boundary in full.

## Compatibility, not neutrality

The invariant promises that the repository stays legible to every tool and every teammate. It does not promise that fufu has no opinions about how you work. Adopting fufu is partly a workflow shift: your branches rebase onto main rather than merging it in, unpublished commits are malleable by default, and force-pushing your own branches — leased and guarded — is routine rather than exceptional.

Those opinions stop at [the push boundary](push-boundary.md). Published history is append-only, and how work lands on the shared branch — merge commit, squash, rebase — remains the team's and the forge's business, not fufu's. Inside your own unpublished work, fufu is opinionated; in everything the rest of the world can see, it is indistinguishable from careful use of plain git. That is the invariant doing its job.
