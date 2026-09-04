# The invariant

**At every instant, the repository is a boring git repository.**

HEAD is attached to a branch. Commits are ordinary commits. `git status` reads the way it always reads. Collaborators, CI, IDEs, and plain-git tooling see nothing unusual, ever. fufu never creates a state that plain git cannot represent. It only automates the moves between states git already has.

This is the one promise that never bends. Every other design question in fufu is settled by asking what preserves it.

You can see it in the mechanisms.

- [Snapshots](snapshots-and-undo.md) live in refs outside the visible graph, so the history you and your teammates read is untouched.
- A [parked change](branches.md) — work fufu sets aside when you leave a branch — is an ordinary stash entry labeled with its branch, sitting in the same stash panel every GUI already has.

jj takes the other road. Its own store is authoritative and the git repository is projected out of it, which is where detached HEADs and machine-generated conflict commits come from. fufu keeps git authoritative and stays inside states git already understands. [fufu vs jj](../comparisons/vs-jj.md) is the full comparison.

## Deleting fufu loses convenience, never data

Because everything fufu writes is ordinary git, removing fufu costs you the automation and nothing else. Every commit, every branch, every snapshot ref, every parked stash entry is still there, still legible, still reachable with plain git commands.

Someone stripped of fufu is slower, not stranded. The stash dance comes back and the manual rebase comes back, but no work is lost and nothing becomes unreadable.

That last part matters as much as the data. A repository fufu has been driving does not need fufu to be explained. A teammate opening it in a GUI sees branches where branches should be, and a stash entry named `fufu: wip on feature-x` next to the button that restores it. Nothing requires the reader to know fufu exists.

## A cache over git, never an authority

The stronger form: fufu is abandonable and returnable at any moment, not merely removable once. A GUI session, a teammate's raw git, a weekend on a machine without fufu — all legitimate, all absorbable when you come back.

Supporting that forces one deep rule. Everything fufu records for itself — the operation log, the rewrite map, parked changes, [held rewrites](held-rewrites.md) — is a cache over git, never an authority.

When fufu's records disagree with what the repository actually contains, the repository wins and fufu rebuilds its picture from what it finds. No state file has to stay consistent for the repository to be valid. The repository is valid on its own, and fufu's records are a derived convenience.

## Reconciliation is loud

Coming back to a repository after working around fufu is reconciliation, not recovery. Nothing is broken and nothing needs repair. fufu compares what it remembered against what it finds, folds the [foreign operations](two-regimes.md#lazy-absorption) into its timeline, and carries on.

What it will not do is quietly forget. A branch that moved, a parked entry that was dropped, a commit that was rewritten behind its back — each is reported, so you know what changed while fufu was not watching.

This is one half of a wider boundary. Work that goes through fufu gets fufu's guarantees; work that goes around it gets git's exact documented behavior, snapshotted and absorbed afterward. [The two regimes](two-regimes.md) covers that boundary in full.

## Compatibility, not neutrality

The invariant promises the repository stays legible to every tool and every teammate. It does not promise fufu has no opinions about how you work.

Adopting fufu is partly a workflow shift. Your branches rebase onto main rather than merging it in, unpublished commits stay malleable by default, and force-pushing your own branches — leased and guarded — is routine rather than exceptional.

Those opinions stop at [the push boundary](push-boundary.md). Published history is append-only, and how work lands on the shared branch — merge commit, squash, rebase — stays the team's business and the forge's, not fufu's.

Inside your own unpublished work, fufu is opinionated. In everything the rest of the world can see, it is indistinguishable from careful use of plain git. That is the invariant doing its job.
