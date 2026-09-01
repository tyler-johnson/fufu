# Adopting fufu

[`ff init`](reference/cli/init.md) in a repository git made means *turn fufu on here*. That is the whole adopt path — no migration, no import, no conversion. This page says what that one command does, what it deliberately does not do, and what you are agreeing to by running it.

## What arming does

Arming writes two things. First, the gc guard: a pair of keys in the repository's local config that stop `git gc` from expiring the refs fufu keeps its snapshots in. Second, [the floor](concepts/snapshots-and-undo.md#the-floor): the operation log's first entry, taken from observed state. `ff undo` reaches back to the floor and no further — everything before fufu's arrival is git's history, not fufu's timeline, and nothing that happened before arming becomes undoable retroactively.

Immediately after the floor, an ordinary capture runs, so whatever the working tree holds at the moment of adoption is already snapshotted before you type anything else. From then on every fufu verb captures the tree before it acts.

`ff init` does not touch your shell or your agent — those are yours, not this repository's. [`ff hook`](reference/cli/hook.md) wires them, and is worth running once per machine: without it capture fires only when you type an `ff` command. [`ff doctor`](reference/cli/doctor.md) reports what is armed and what is wired.

## What does not change

Nothing about the repository that anyone else can see. Refs, history, remotes, hooks, CI, and teammates all continue exactly as before, because [the invariant](concepts/invariant.md) holds from the first moment: at every instant the repository is a boring git repository. Arming adds config keys and refs in fufu's own namespace; it rewrites nothing, moves nothing, and installs no hooks that intercept anything. A teammate cloning the repository, a GUI opening it, a CI job checking it out — none of them can tell fufu is there.

## The workflow shift

Adopting fufu is partly a workflow shift, not a transparent overlay. Using it is accepting a set of positions: your branches rebase onto main rather than merging it in, unpublished commits are malleable by default, and force-pushing your own branches — leased and guarded — is routine rather than exceptional. If your habits are merge-from-main and history-is-immutable-once-committed, fufu will pull against them.

Those opinions stop at [the push boundary](concepts/push-boundary.md). Published history is append-only, and how work lands on the shared branch — merge commit, squash, rebase — remains the team's and the forge's business, not fufu's. The shift is entirely inside your own unpublished work.

## Trying it and leaving

fufu is abandonable and returnable at any moment, and deleting it loses convenience, never data. Everything fufu writes is ordinary git: snapshots are refs outside the visible graph, parked changes are labeled stash entries, and the operation log is a cache over the repository, never an authority over it. Uninstall the binary and the repository is complete and legible without it — the stash dance comes back, the manual rebase comes back, but no commit, no branch, and no file state is lost.

Leaving does not have to be permanent, and it does not have to be total. A GUI session, a weekend of raw git, a machine without fufu installed — all are absorbed when you return: the first fufu operation back compares what it remembered against what it finds, folds the difference into the log as foreign operations, and says out loud anything that no longer matches. [The two regimes](concepts/two-regimes.md) covers that boundary in full; the short version is that coming back is reconciliation, not recovery.

## Adopting mid-flight

`ff init` does not ask for a clean state, because the repository's current state is exactly what the floor records.

**A dirty tree** is fine. Nothing is touched at arming, and the capture that follows the floor snapshots the uncommitted work immediately, so it is held from the first moment.

**An in-progress rebase or merge** stays git's. fufu does not adopt, continue, or abort it — it belongs to the outside regime, so finish it or abort it with git as you would have anyway, and the resulting motion is absorbed as a [foreign operation](concepts/two-regimes.md#lazy-absorption) at your next fufu verb.

**Existing stashes** are never touched. fufu only ever applies stash entries it created itself, identified by the exact commit sha it recorded — never selected by message, position, or base. Your own stashes stay in `git stash list` untouched, alongside any parked entries fufu adds later.
