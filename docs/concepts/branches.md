# Branches

**Every line of work is an ordinary git branch, and no work waits for a name.**

[`ff start`](../reference/cli/start.md) begins every new line of work on a fresh branch. Bare, it forks from trunk; a revision argument forks there instead. The [open change](changes.md) parks with the branch you are leaving, and the new branch opens clean — nothing ever crosses a fork. The verbs divide the ground the same way everywhere: `ff commit` records, `ff switch` resumes, `ff start` begins.

## Minted names

You do not name a branch at `ff start` unless you want to. Every start mints an **anonymous branch**: a real branch with a generated petname under a reserved prefix, like `ff/hidden-wren`. It is a genuine ref under `refs/heads/` from the moment it exists — every GUI shows it, every git command addresses it, and no push refspec matches it by accident. That is [the invariant](invariant.md) applied to naming: fufu does not hold work in some unnamed limbo of its own; it puts the work on a branch git already understands and defers only the christening.

This ordering matches how work actually goes. You start a spike before you know whether it is a refactor, a fix, or a dead end; the name comes when the work has earned one. If you do know at birth, `ff start -b hotfix` names the minted branch on the spot.

## Claiming a name

[`ff describe -b <name>`](../reference/cli/describe.md) names the branch you are on. Naming lives on `describe` because that verb's job is saying what work is: `-m` sets the change's description, `-b` sets the branch's name, one verb for both axes. Claiming a petname is the same act as replacing a name you chose earlier — there is no separate rename command to learn.

The rename carries everything fufu associates with the branch: the capture chain of [snapshots](snapshots-and-undo.md), any parked change, and the pending description. This is the part a bare `git branch -m` would orphan — git renames the ref, but the stash entry labeled with the old name and fufu's records keyed to it would be left pointing at a branch that no longer exists. Going through the fufu verb keeps the whole bundle attached; that is [the two regimes](two-regimes.md) in miniature.

Claiming a name is also the natural "this is real now" gesture. An anonymous branch is fine to work on indefinitely, but the moment work heads for a remote, giving it a real name is the step that marks it as something the rest of the world will see.

## Every commit lands on a branch

HEAD never detaches under fufu. Every commit lands on some branch, and the branch tip advances as commits land, because that is git's own behavior once HEAD is attached — fufu does not move branches itself so much as keep you in the state where git moves them for you. Even operations that in raw git would detach HEAD, like editing a commit deep in history, instead mint an anonymous branch at the target and switch to it.

This is [the invariant](invariant.md) at work. A detached HEAD is a state plain-git tooling handles badly and teammates read with alarm; by never entering it, fufu keeps the repository legible at every instant. Contrast jj's bookmarks, which sit still until told to move: fufu's branches behave exactly as git branches because they are git branches, with nothing projected or simulated.

## Listing and deleting

[`ff branch list`](../reference/cli/branch-list.md) — bare `ff branch` is the same list — shows named branches first, then the anonymous ones, kept apart so a petname never reads as something you chose. Each row carries the branch's tip, the subject there, and what is hanging off it: a parked change, a pending description, and how the branch stands against its upstream. Below your own branches come the ones a remote holds that you do not — rows you cannot switch to directly, because `ff switch` resolves local names only. `ff start origin/spike` is the verb that forks one of those into a branch here.

[`ff branch delete`](../reference/cli/branch-delete.md) removes a branch without a merged-check to argue with, because it does not need one: the branch's pointer moves to trash rather than evaporating, its parked change is demoted to an ordinary stash entry, and the tip stays pinned by the operation. `ff undo` brings the branch and its timeline back. A published branch has a second half — the copy on the remote — which a plain delete leaves standing and says so; `--shared` removes that copy too, under a lease. The remote half is the one thing undo cannot reach, which is why removing it takes an explicit flag.

## Switching by prefix

[`ff switch`](../reference/cli/switch.md) takes a branch name or any unique prefix of one — `ff switch uni` reaches `unicode-cleanup` if nothing else starts that way. An ambiguous prefix is an error that lists the candidates, never a guess. What happens to the open change on either side of the move — parked here, resumed there — is the change lifecycle, covered in [changes](changes.md).

## Stacking: a branch records its parent

`ff start <branch> -b <name>` forks from another branch's tip and records that branch as the new one's parent; a bare `ff start` forks from trunk and records nothing, so its base is trunk wherever trunk goes. That record is what the base means everywhere fufu says the word: the base axis on `ff status`, the standing `ff branch list` reports, and the replay every rewrite performs. [`ff restack --onto`](../reference/cli/restack.md) is the one way to change it.

When a branch's tip moves, the branches stacked on it follow. [`ff restack`](../reference/cli/restack.md), [`ff sync`](../reference/cli/sync.md), [`ff absorb`](../reference/cli/absorb.md), [`ff lift`](../reference/cli/lift.md), [`ff describe <rev>`](../reference/cli/describe.md), and [`ff done`](../reference/cli/done.md) each replay every local branch whose base is the branch they moved onto its new tip, parent before child, through the whole tree, inside the verb's own operation, so one `ff undo` takes the rewrite and the cascade back together. Each replay is performed, not predicted: a branch whose replay conflicts is [held](held-rewrites.md) where it stands, the branches above it stay put because their base did not move, and `ff resolve` on that branch followed by `ff done` resumes the cascade from there. A branch checked out in another worktree is skipped and the worktree named, since only that worktree may move its HEAD; one already holding a rewrite, and one whose commits hold a merge, are skipped the same way. A branch with no commits of its own stays put. The verb says what followed, what held, and what was skipped. `ff restack` and `ff sync` exit 3 when any branch in the run held, because the question they answer is whether the stack is lined up; the rewriting verbs exit 0, because the rewrite they were asked for landed, and `ff status` shows the hold. [Stacked changes](../guides/stacked-changes.md) walks a stack through review.

## Sync covers the whole repository

[`ff sync`](../reference/cli/sync.md) fetches once and brings every local branch up to date with both things it answers to: first the shared copy of each branch, then the base beneath each, parent before child, so a trunk that moved carries every branch started from it in one run. Standing on a branch changes nothing about how it is treated; it only decides whether a working tree moves, and the branches you are not on move as refs and objects and touch no file. The whole run is one operation and one `ff undo`. [The push boundary](push-boundary.md) covers what sync takes in and what publish sends.

## Tracking: one branch, one shared copy

A branch answers to at most one remote, and its shared copy there is the only one. fufu does not model a branch published to two places, because the guarantees around publishing — the lease, rollback, knowing which commits out there are yours — all assume a single shared copy to reason about.

Most repositories never face the question: with a single remote, or one named `origin`, the first [`ff publish`](../reference/cli/publish.md) creates the shared copy and sets up tracking in the same step. With several remotes, `ff publish --to <remote>` names where this branch answers and records the answer, so every later `ff publish`, `ff sync`, and `ff status` needs no flag. Asking `--to` for a branch that already answers somewhere else is refused — the answer is a fact about the branch, given once. What publishing actually promises, and the lease that guards it, is [the push boundary](push-boundary.md).
