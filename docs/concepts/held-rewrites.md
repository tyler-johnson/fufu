# Held rewrites

**A conflict is the operation staying pending, not a strange object in the graph.**

fufu's rewrites all run in memory and land only when the result is clean: [`ff restack`](../reference/cli/restack.md) moving a branch onto a new base, [`ff sync`](../reference/cli/sync.md)'s replay, [`ff done`](../reference/cli/done.md) landing an editing session, and the restacking that [`ff absorb`](../reference/cli/absorb.md) and [`ff lift`](../reference/cli/lift.md) do to descendants.

When a step conflicts, nothing is touched:

- No ref moves.
- No half-applied tree reaches the working directory.
- No rebase sits stopped in the repository, and both inputs stay ordinary git commits.

What gets recorded instead is the intent — this branch has a pending rewrite, conflicting at commit such-and-such — as a **held rewrite**. The verb reports the hold, says what conflicts and where, and exits 3, the code meaning a human decision is required.

What that buys is scheduling. The conflict does not interrupt you at the machine's moment. You keep working at the existing tip and materialize the conflict when you choose.

Someone who stays on the fufu surface never meets a conflict at a moment they did not choose. [The two regimes](two-regimes.md) makes that promise, and holds are how it is kept.

## Why not a conflicted commit

jj answers the same problem the other way. It stores the unresolved conflict inside the result: a commit whose content is a symbolic merge expression, materialized as markers on demand.

That machinery exists mostly so jj's always-rebasing engine never has to stop, and it cannot cross [fufu's invariant](invariant.md). A git tree cannot hold an expression, so a `.jjconflict-*`-style tree is exactly the kind of state plain git cannot read, and therefore the kind fufu refuses to write.

fufu's observation is that for a person, conflicts are operation-shaped rather than edit-shaped. The user-visible benefit of jj's model is the deferral, and the deferral survives translation into states git already understands.

Instead of a strange commit that exists, you get a pending rewrite that does not yet. A conflict is a pending decision, not a commit, and the graph never contains anything a teammate's GUI cannot display.

## What a hold records

A hold records the verb's own question — the branch, the target, what it was asked to become. It never records the plan it could not finish computing.

Every input is a ref or the working tree, so nothing has to be pinned, and resolving is a recomputation rather than a comparison. That is [cache-not-authority](invariant.md#a-cache-over-git-never-an-authority) taken literally, and it is what makes a hold durable rather than fragile.

So you keep committing at the existing tip, and the pending rewrite replays over whatever you add, because the replan sees what you added. If the world moves such that the rewrite now applies cleanly, the hold is released rather than resolved.

A target that has gone, or moved out of history — foreign commits, a rewritten base — expires the hold loudly, at the moment somebody asks. It is never silently replayed from a stale plan.

## `ff resolve`: all of it at once

Git's stop-fix-continue rebase is sequential because each replayed commit changes the base of the next.

fufu runs that propagation in memory instead. Each step of the held rewrite replays against the previous step's result, unresolved regions are carried forward as literal marker content, and a commit whose own changes land clear of the marks replays over them untouched. A conflict a later commit resolves anyway vanishes along the way.

### The session

[`ff resolve`](../reference/cli/resolve.md) then puts every surviving conflict region into the working tree together, as ordinary conflict markers, in one editing session.

The current side is labeled `the rewrite so far`. The incoming side carries the step that wrote it — `>>>>>>> rebasing "add parser options" (3/10)` — because the incoming side is where git puts the commit, and therefore where a reader already looks. Those labels are not decoration: they are what attributes each fix back to its owning step when the session lands.

Nothing moves when the session opens. Your branch stays put, a [parked change](changes.md) — work set aside with another branch — waits where it was, and the hold stays, because it is what the session is resolving.

### Landing the session

Fix the markers, then `ff done` lands it. Each resolution is folded back into the step that wrote it, the chain of steps re-runs in memory, and the whole rebased stack lands at once — refs move one time, every landed commit clean, no conflicted state ever existing in the graph.

Two commits conflicting on the same region is the one shape this cannot flatten. Carried markers do not nest — they interleave, and the earlier block stops bracketing anything.

So the chain stops rather than write the tangle. `ff resolve` presents the steps before it, and what is left is held again. A stack of tangles unwinds one round at a time, without anyone having to know the word.

`ff resolve --abandon` drops the hold instead, and an open session's markers with it. Either way the session is an operation like any other, so one [`ff undo`](snapshots-and-undo.md) takes it back — markers, resolutions, all of it.

## Deferred requires loud

Deferring a conflict is only safe if you cannot forget it. Holds get three disciplines for that.

- **A hold is announced at creation.** The verb says what conflicts and where before it exits.
- **A hold is pinned until it is gone.** [`ff status`](../reference/cli/status.md) shows a `held:` line naming the verb, the commit it stopped at, the conflicting files, and the way out, on every render until the rewrite lands or is abandoned. Once a session is open, a `resolving:` line stands above it, because markers in your working tree are the more urgent fact.
- **Exits are blocked**, which is the next section.

[`ff branch list`](../reference/cli/branch-list.md) marks a held branch the same way it marks an unfinished session, so standing work is visible wherever branches are listed.

Deferred and quiet is how work rots. The disclosure is what makes the deferral safe.

## What a hold blocks, and what it does not

A hold blocks [`ff publish`](push-boundary.md). Nothing is sent while the branch's commits are still about to be rewritten out from under it.

That guard lives on the fufu surface only. Raw `git push` is git and it pushes, with the status channel getting loud afterward rather than a hook getting in the way — exactly as [the two regimes](two-regimes.md) says.

A hold blocks nothing local. You can commit, switch, park, and keep building at the existing tip. A rewrite that cannot leave the machine is still one you can keep working on.

What fufu gives up against jj is the other half. You cannot build on the post-rewrite state before resolving, and conflicted commits cannot be shipped around — which is the half you would never want to push anyway.
