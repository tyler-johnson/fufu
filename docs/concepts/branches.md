# Branches

**Every line of work is an ordinary git branch, and no work waits for a name.**

[`ff start`](../reference/cli/start.md) begins every new line of work on a fresh branch. Bare, it forks from trunk — your main line of development. Give it a revision and it forks there instead.

The [open change](changes.md) — the edits sitting in your working copy — parks with the branch you are leaving, and the new branch opens clean. Nothing ever crosses a fork. The verbs divide the ground the same way everywhere: [`ff commit`](../reference/cli/commit.md) records, `ff switch` resumes, `ff start` begins.

## Minted names

You do not name a branch at `ff start` unless you want to. Every start mints an **anonymous branch**: a real branch with a generated petname under a reserved prefix, like `ff/hidden-wren`.

It is a genuine ref under `refs/heads/` from the moment it exists. Every GUI shows it, every git command addresses it, and no push refspec matches it by accident.

That is [the invariant](invariant.md) applied to naming. fufu does not hold work in an unnamed limbo of its own. It puts the work on a branch git already understands and defers only the christening.

The ordering matches how work actually goes. You start a spike before you know whether it is a refactor, a fix, or a dead end, and the name comes when the work has earned one. If you do know at the outset, `ff start -b hotfix` names the branch on the spot.

## Claiming a name

[`ff describe -b <name>`](../reference/cli/describe.md) names the branch you are on. Naming lives on `describe` because that verb's job is saying what work is: `-m` sets the change's description, `-b` sets the branch's name. Claiming a petname is the same act as replacing a name you chose earlier, so there is no separate rename command to learn.

The rename carries everything fufu associates with the branch — the chain of [snapshots](snapshots-and-undo.md) taken before each action, any parked change, and the pending description.

This is the part a bare `git branch -m` would orphan. git renames the ref, but the stash entry labeled with the old name, and fufu's records keyed to it, would be left pointing at a branch that no longer exists. Going through the fufu verb keeps the whole bundle attached. That is [the two regimes](two-regimes.md) in miniature.

Claiming a name is also the natural "this is real now" gesture. An anonymous branch is fine to work on indefinitely, but once work heads for a remote, a real name is what marks it as something the rest of the world will see.

## Every commit lands on a branch

HEAD never detaches under fufu. Every commit lands on some branch, and the branch tip advances as commits land, because that is git's own behavior once HEAD is attached.

fufu does not so much move branches itself as keep you in the state where git moves them for you. Even operations that in raw git would detach HEAD, like editing a commit deep in history, instead mint an anonymous branch at the target and switch to it.

This is [the invariant](invariant.md) at work. A detached HEAD is a state plain-git tooling handles badly and teammates read with alarm. By never entering it, fufu keeps the repository legible at every instant.

Contrast jj's bookmarks, which sit still until told to move. fufu's branches behave exactly as git branches because they are git branches, with nothing projected or simulated.

## Listing and deleting

[`ff branch list`](../reference/cli/branch-list.md) — bare [`ff branch`](../reference/cli/branch.md) is the same list — shows named branches first, then the anonymous ones. They are kept apart so a petname never reads as something you chose.

Each row carries the branch's tip and the subject there, plus what is hanging off it:

- a parked change — the edits set aside when you switched away
- a pending description
- how the branch stands against its shared copy on the remote

Below your own branches come the ones a remote holds and you do not. You cannot switch to those directly, because `ff switch` resolves local names only. `ff start origin/spike` is the verb that forks one of them into a branch here.

[`ff branch delete`](../reference/cli/branch-delete.md) removes a branch with no merged-check to argue with, because it does not need one. The branch's pointer moves to trash rather than evaporating, its parked change is demoted to an ordinary stash entry, and the tip stays pinned by the operation. [`ff undo`](../reference/cli/undo.md) brings the branch and its timeline back.

A published branch has a second half — the copy on the remote — which a plain delete leaves standing, and says so. `--shared` removes that copy too, under a lease: the removal goes through only if the remote copy still stands where you last saw it. The remote half is the one thing undo cannot reach, which is why removing it takes an explicit flag.

## Switching by prefix

[`ff switch`](../reference/cli/switch.md) takes a branch name or any unique prefix of one. `ff switch uni` reaches `unicode-cleanup` if nothing else starts that way, and an ambiguous prefix is an error listing the candidates rather than a guess.

What happens to the open change on either side of the move — parked here, resumed there — is the change lifecycle, covered in [changes](changes.md).

## Stacking: a branch records its parent

`ff start <branch> -b <name>` forks from another branch's tip and records that branch as the new one's **base**. A bare `ff start` forks from trunk and records nothing, so its base is trunk wherever trunk goes.

That record is what "base" means everywhere fufu says the word: the base axis on [`ff status`](../reference/cli/status.md), the standing `ff branch list` reports, and the replay every rewrite performs. [`ff restack --onto`](../reference/cli/restack.md) is the one way to change it.

### The cascade

When a branch's tip moves, the branches stacked on it follow. Six verbs move a tip and set that cascade going:

- [`ff restack`](../reference/cli/restack.md)
- [`ff sync`](../reference/cli/sync.md)
- [`ff absorb`](../reference/cli/absorb.md)
- [`ff lift`](../reference/cli/lift.md)
- [`ff describe <rev>`](../reference/cli/describe.md)
- [`ff done`](../reference/cli/done.md)

Each replays every local branch whose base is the branch it moved, parent before child, through the whole tree. It happens inside the verb's own operation, so one `ff undo` takes the rewrite and the cascade back together.

Each replay is performed, not predicted. A branch whose replay conflicts is [held](held-rewrites.md) where it stands, and the branches above it stay put because their base did not move. [`ff resolve`](../reference/cli/resolve.md) on that branch, then `ff done`, resumes the cascade from there.

Three kinds of branch are skipped rather than replayed, and the verb names each one:

- a branch checked out in another worktree, since only that worktree may move its HEAD
- one already holding a rewrite
- one whose commits hold a merge

A branch with no commits of its own stays put.

The verb says what followed, what held, and what was skipped. `ff restack` and `ff sync` exit 3 when any branch held, because the question they answer is whether the stack is lined up. The rewriting verbs exit 0, because the rewrite they were asked for landed, and `ff status` shows the hold.

[Stacked changes](../guides/stacked-changes.md) walks a stack through review.

## Sync covers the whole repository

[`ff sync`](../reference/cli/sync.md) fetches once and brings every local branch up to date with both things it answers to: first the shared copy of each branch, then the base beneath it, parent before child. A trunk that moved therefore carries every branch started from it, in one run.

Standing on a branch changes nothing about how it is treated. It only decides whether a working copy moves — the branches you are not on move as refs and objects and touch no file.

The whole run is one operation and one `ff undo`. [The push boundary](push-boundary.md) covers what sync takes in and what publish sends.

## Tracking: one branch, one shared copy

A branch answers to at most one remote, and its shared copy there is the only one. fufu does not model a branch published to two places, because the guarantees around publishing — the lease, rollback, knowing which commits out there are yours — all assume a single shared copy to reason about.

Most repositories never face the question. With a single remote, or one named `origin`, the first [`ff publish`](../reference/cli/publish.md) creates the shared copy and sets up tracking in the same step.

With several remotes, `ff publish --to <remote>` names where this branch answers and records the answer, so every later `ff publish`, `ff sync`, and `ff status` needs no flag. Asking `--to` for a branch that already answers somewhere else is refused: the answer is a fact about the branch, given once.

What publishing actually promises, and the lease that guards it, is [the push boundary](push-boundary.md).
