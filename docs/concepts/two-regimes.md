# The two regimes

fufu's guarantees follow its surface. Inside it, jj's rules apply; outside it, git's rules apply — exactly. Every operation belongs to one regime or the other, and knowing which is knowing precisely what to expect from it.

## Inside: through fufu

An operation that goes through fufu gets everything fufu promises. The working tree is [snapshotted](snapshots-and-undo.md) before the operation runs, so nothing it does can lose file state. The operation lands in the operation log, so one `ff undo` takes it back — refs and working tree together. Switching branches [parks](branches.md) dirty work, so it resumes with its branch. Syncing replays in memory and [holds its conflicts](held-rewrites.md) for a moment you choose. And `ff status` reports futures, not just facts: not "12 commits behind main" but "rebases cleanly onto main," computed in memory before you commit to anything.

The user who stays on the fufu surface never meets a conflict at a moment they didn't choose. That is what "inside" buys.

## Outside: around fufu

Everything else — a GUI's branch switcher, a raw `git pull` in another terminal, an IDE's commit button, a teammate's push, a script that shells out to git — is outside. Outside, you get git's exact documented behavior, including git's conflicts at git's usual moments. That is expected, and it belongs to you; fufu does not reach into operations it did not perform. There are no hooks that intercept, no wrappers that second-guess, no state that a foreign write can corrupt.

Guards obey the same boundary. `ff sync` refuses to publish a stack with held rewrites, because that guard is a property of fufu's verb. Raw `git push` is git, and it pushes; the status channel gets loud after the fact rather than a hook getting in the way.

This makes GUIs and IDEs first-class writers, not tolerated exceptions. Every git GUI keeps working identically — showing status, making commits, switching branches — because fufu's conveniences accrue per-operation to whoever goes through fufu, and cost nothing to whoever doesn't.

The boundary is execution path, not spelling. The recommended shell alias, `alias git='ff git'`, moves typed git onto the fufu surface — captured first, absorbed as fufu's own — while anything that resolves git on PATH stays foreign. And automation is not foreign by nature, only by habit: a script, a CI job, or an agent that calls `ff` is inside the surface with everyone else and gets everything the surface promises.

## Lazy absorption

fufu does not watch the repository. Foreign motion is noticed lazily, at the next fufu operation: fufu compares what it remembered against what the repository now says, and folds the difference into the operation log as a foreign operation — labeled as foreign, quoted with git's own reflog messages, and undoable like anything fufu did itself. `ff undo` can therefore reach past fufu's own operations into things done behind its back, because by the time you ask, they are in the log.

Absorption is loud. The foreign operation is reported in `ff status`, and the notice stays pinned there while the log's tip is foreign, so motion fufu did not perform is never silently blended into motion it did. Anything fufu remembered that reality no longer matches — a branch that moved, a parked entry that was dropped by hand — gets said out loud, then the records update to match the repository. The repository wins every disagreement; [the invariant](invariant.md) explains why fufu's records are a cache over git and never an authority.

## A weekend without fufu

The strong form of the outside regime: you can leave fufu entirely — a GUI session, a laptop that doesn't have it installed, a weekend of raw git — and come back. Nothing accumulates, nothing breaks, nothing needs repair. Returning is reconciliation, not recovery: at your first fufu operation back, everything that happened in the meantime is observed, reported, and absorbed into the timeline, and the surface's guarantees resume from there.

This is only safe because of [the invariant](invariant.md). At every instant the repository is a boring git repository, so nothing done to it with plain git can put it in a state fufu cannot comprehend — there is no fufu-shaped consistency for foreign operations to violate. The two regimes are the invariant seen from the operational side: inside, automation you can undo; outside, git exactly; and a loud, mechanical reconciliation whenever you cross back over.
