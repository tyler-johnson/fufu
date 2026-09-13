# The two regimes

fufu's guarantees follow its surface. Work that goes through fufu gets fufu's rules; work that goes around it gets git's rules, exactly. Every operation belongs to one regime or the other, and knowing which one is knowing what to expect from it.

## Inside: through fufu

An operation that goes through fufu gets everything fufu promises:

- Repository commands and active hooks take [snapshots](snapshots-and-undo.md), subject to coverage, capture success, and retention.
- Recorded local changes can be restored with [`ff undo`](../reference/cli/undo.md) — refs and working copy together, on this worktree's chain. Remote updates and ambient maintenance are outside that undo scope.
- Switching branches [parks](branches.md) dirty work, so it resumes with its branch.
- Pulling replays in memory and [holds its conflicts](held-rewrites.md) for a moment you choose.
- [`ff status`](../reference/cli/status.md) reports futures rather than just facts: not "12 commits behind main" but "rebases cleanly onto main," worked out in memory before you commit to anything.

Someone who stays on the fufu surface never meets a conflict at a moment they did not choose. That is what "inside" buys.

## Outside: around fufu

Everything else is outside:

- a GUI's branch switcher
- a raw `git pull` in another terminal
- an IDE's commit button
- a teammate's push
- a script that shells out to git

Outside, you get git's exact documented behavior, including git's conflicts at git's usual moments. That is expected, and it belongs to you. fufu does not reach into operations it did not perform: no hooks that intercept, no wrappers that second-guess, no state a foreign write can corrupt.

[`ff push`](../reference/cli/push.md) refuses to send a branch with a held rewrite. Raw Git that bypasses fufu has no such guard. Git invoked through the shell alias or an active agent hook is also subject to `fufu.gitPolicy`; strict policy can refuse it.

This makes GUIs and IDEs first-class writers rather than tolerated exceptions. Every git GUI keeps working identically — showing status, making commits, switching branches — because fufu's conveniences accrue to whoever goes through fufu, one operation at a time, and cost nothing to whoever does not.

### Which program ran

The recommended shell alias, `alias git='ff git'`, routes typed Git through [`ff git`](../reference/cli/git.md). A strict refusal happens before capture; otherwise it attempts capture before running Git. Capture failure warns and does not stop Git. A script or editor that bypasses the alias needs another active integration to capture before its edits.

Automation is not foreign by nature, only by habit. A script, a CI job, or an agent that calls `ff` is inside the surface with everyone else, and gets everything the surface promises.

## Lazy absorption

fufu does not watch the repository. It notices foreign motion at the next fufu operation, by comparing what it remembered against what the repository now says.

Ref differences are folded into the operation log as foreign operations, with Git's reflog messages. Undo can recover recorded ref and file states, but ref history cannot reconstruct uncaptured edits destroyed by an outside command.

Absorption is loud. The foreign operation is reported in `ff status`, and the notice stays pinned there while the log's tip is foreign, so motion fufu did not perform is never quietly blended into motion it did.

Anything fufu remembered that reality no longer matches — a branch that moved, a parked change whose tip moved under it — is said out loud, and then the records update to match the repository. The repository wins every disagreement. [The invariant](invariant.md) explains why fufu's records are a cache over git and never an authority.

## A weekend without fufu

Here is the strong form of the outside regime. You can leave fufu entirely — a GUI session, a laptop without it installed, a weekend of raw git — and come back. Nothing accumulates, nothing breaks, nothing needs repair.

Returning is reconciliation, not recovery. At your first fufu operation back, everything that happened in the meantime is observed, reported, and absorbed into the timeline, and the surface's guarantees resume from there.

This is only safe because of [the invariant](invariant.md). At every instant the repository is a boring git repository, so nothing done with plain git can put it in a state fufu cannot make sense of. There is no fufu-shaped consistency for a foreign operation to violate.

The two regimes are that invariant seen from the operational side: inside, automation you can undo; outside, git exactly; and a loud, mechanical reconciliation whenever you cross back over.
