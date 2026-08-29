# Architecture

**fufu is three floors, and each floor is what licenses the one above it.** Capture makes loss impossible, futures make outcomes knowable before anything is spent, and the verbs move the repository between ordinary git states. This page is the contributor's tour of how that stands in the code and on disk. The reader-facing story lives in the [concepts section](../concepts/invariant.md), and the argumentative founding text is the [design document](design.md) — where the two disagree, this page follows the code.

In the source, the floors map roughly to modules in `crates/ff-core/src`: capture is `snapshot/` and `ops/`, futures is `futures.rs`, and the verb floor is `rewrite.rs`, `restack.rs`, and the per-verb modules around them.

## Floor 1 — capture

Every working-tree state is snapshotted before anything acts on it. Every ff verb rides a pre-command capture lane before its own work; `ff git` captures and then hands the arguments to git verbatim; and the hooks [`ff hook`](../reference/cli/hook.md) installs make capture ambient rather than something anyone remembers — before every tool call a wired agent makes, before every git command typed through the shell alias, and at every shell prompt, all arriving through [`ff trigger`](../reference/cli/trigger.md) with the source named. [Snapshots and undo](../concepts/snapshots-and-undo.md) is the reader-facing account of what this buys.

A snapshot is not a second concept with its own log: **a snapshot is what an operation carries.** Each worktree has one operation log, a chain of commits at `refs/fufu/wt/<id>/ops`, and every capture and every mutating verb appends to it. An operation commit's tree is the worktree at the end of the operation. Its first parent is the previous operation, so a first-parent walk is the log; its second parent is the commit HEAD stood on, which keeps the user's real history reachable from the chain and is the whole of fufu's gc pin. A verb's operation additionally hangs a parentless record commit off itself — `op.json`, the full ref table, and the index tree — plus extra parents pinning every sha its ref transitions touch. A capture carries no record at all, because a capture changes no ref by invariant, and that invariant is what keeps the highest-volume path in the tool storage-neutral instead of storage-doubling.

Verb operations are written write-ahead: the operation records its planned end state on all four axes — refs, tree, index, HEAD — before the mutation runs. That is why undo is one lookup rather than three, and why re-running after a crash converges: the plan is a state, not a script. `ff undo` itself is a pointer move along the chain, never an append — the log records work and never navigation — and what the pointer steps off stays reachable through the ref's own reflog, which fufu guards by writing `reflogExpire=never` for `refs/fufu/*` into the repository config once.

Absorption is the floor's third job. Every mutating verb's preamble reconciles first and captures second: motion that happened around fufu — raw git, a GUI, an IDE — is absorbed as a foreign operation before the verb records "the state before this verb", so that state is one fufu actually agreed to. A gap of foreign motion collapses into a single operation with restore points at its endpoints, because git's reflogs record where refs moved but never what the tree held between moves. [The two regimes](../concepts/two-regimes.md) covers that boundary from the user's side.

## Floor 2 — futures

The second floor answers what an operation would cost before anyone spends it. A rebase is a replay, so `futures::probe` simulates one: every commit of `base..tip` is re-applied onto a moving cursor as an in-memory three-way tree merge, and when the branch is the one underfoot, the open change replays as a final step — so a rebase that would conflict in uncommitted work is caught one step further out than the commits. The whole replay runs inside one object-memory clone of the repository that is dropped with the answer, so a probe writes nothing: a repository that has never run one is byte-identical to one that has run a thousand.

The verdict set is closed. A replay comes back up-to-date, fast-forward, clean (counting the commits that would be dropped as emptied), or conflicting — naming the commit that breaks and the paths it breaks in. Where a wrong answer is possible the answer is an honest unknown instead: unrelated histories, merge commits in the range (rebase semantics for a merge are ambiguous, and fufu declines to pick a side), or a range past `fufu.futuresDepth` (default 200 — the cap exists because status probes at prompt rate, and a verb somebody typed pays the real cost instead). The remote axis adds three shapes of its own: gone, never published, and undone — the shared copy standing exactly where this repository last left it, with the branch since stepped back.

A branch answers to two things, measured as two independent axes: the base beneath it — an explicitly recorded parent branch, else trunk — and the remote copy of itself. Both are futures over the same probe; only the wording differs.

The cache is one plain JSON file per branch at `<common-dir>/fufu/futures/<branch>`, holding one slot per axis, and each slot is keyed by its own four inputs: the ref measured against, that ref's tip, the branch tip, and the open change's tree. The key is the invalidation — a stale entry is by definition one that will not be used — so there is no eviction policy and no staleness clock, and deleting the file changes no answer, only the cost of getting one. (The design document's substrate section describes the cache as keyed by `(base, ours, theirs)` and recomputing only when a ref moves; the shipped key is the four inputs above, so a probe also recomputes when only the open change moved.)

Where the answer is spent is deliberate. [`ff status`](../reference/cli/status.md) reports futures, not just facts — "main moved — rebases cleanly (3 commits replayed)" before anything moves, or the commit and files a rebase would break on. The bare `ff` map and `ff branch list` deliberately do not pay a merge simulation per row: verdicts belong to status, and the most-typed commands must stay flat.

## Floor 3 — the verbs

Every verb is a transition between boring git states — that is [the invariant](../concepts/invariant.md) stated as an implementation rule. A verb's shape is the same everywhere: reconcile, capture, write the operation ahead, then move refs. HEAD stays attached throughout; nothing a verb produces requires knowing fufu exists to read.

The floor's theme is land-if-clean: operations attempt themselves speculatively with the same simulation floor 2 exposes. Clean means refs move and status says what happened; not clean means nothing is touched and the operation becomes a [held rewrite](../concepts/held-rewrites.md) — announced at creation, pinned in status until resolved, and released through `ff resolve` or by undoing the operation that held.

One rewrite engine (`rewrite.rs`) serves every rewrite verb rather than each forking its own commit-writing logic. A rewrite that moves no tree — a reword — re-parents commits without replaying them; a rewrite that moves a tree replays by three-way merge, the writing half of exactly what the probe simulates. Every rewrite records its old→new map as a field on the operation, so the log's pins and [`ff trim`](../reference/cli/trim.md)'s retention cover the map for free; and no empty commit survives a replay — a commit whose replayed tree matches its new first parent introduces nothing, is not written, and is announced rather than silently dropped.

[`ff restack`](../reference/cli/restack.md) is the primitive under the floor — replay these commits onto that base, hold on conflict — and the other verbs are aims for it: [`ff sync`](../reference/cli/sync.md) runs it against both of a branch's axes with the network in front, and `ff done` is restack pointed at an edit session's parent. The one act automation never chains into is publishing: a push leaves the machine, so [`ff publish`](../reference/cli/publish.md) is always a verb a person types — [the push boundary](../concepts/push-boundary.md) from the mechanism's side.

## Where fufu's state lives

Everything fufu writes lives in two places: refs under `refs/fufu/`, and plain files under `<common-dir>/fufu/`.

The refs:

| Ref | What it holds |
| --- | --- |
| `refs/fufu/wt/<id>/ops` | One worktree's operation chain — the log itself. `<id>` is the gitdir basename git files the worktree under; the main worktree's is `main`. |
| `refs/fufu/wt/<id>/trash/@ops` | That chain's pre-trim tip — the last trim's own undo. |
| `refs/fufu/snap/<branch>` | A pointer to the newest operation on that branch, moved in the same transaction as the chain tip. |
| `refs/fufu/parked/<branch>` | The sha of the branch's parked stash entry. The entry itself is an ordinary git stash, visible in every stash panel; the ref is how fufu finds the exact entry again. |
| `refs/fufu/published/<branch>` | The tip this repository last left the shared copy standing at. Deliberately a ref rather than a log entry, because `ff undo` is a pointer move and must not rewind the one fact it cannot reverse — where the wire was left. |
| `refs/fufu/trash/<branch>` | A deleted branch's tip, kept by retention. |
| `refs/fufu/legacy/*` | Pre-cutover logs, parked as a receipt when fufu takes over a repository that still holds them. |

Operation chains live in the shared ref namespace rather than under `refs/worktree/`, and that is a measurement rather than a preference: `git gc` run from the main worktree collects objects pinned only by a linked worktree's worktree-local refs, and reachability is fufu's entire gc pin. The same fact is what lets a chain outlive the worktree it belonged to, which is the point — a deleted worktree's work stays addressable through the same `ff op` verbs as anything else.

The files:

| Path | What it holds |
| --- | --- |
| `<common-dir>/fufu/futures/<branch>` | The futures cache described above. |
| `<common-dir>/fufu/branch/<branch>` | Branch metadata: the pending description, the explicitly recorded parent branch, the fork point, an open edit session, a held rewrite, an open resolution. Empty metadata deletes the file. |
| `<common-dir>/fufu/ops/<chain>/live`, `…/trash` | The operation id index — a sorted file of op ids per domain, derived from the chain. |
| `<common-dir>/fufu/oplog-<chain>.lock` | The write lock on one chain. |

Every piece is disposable, in one of two grades. The futures cache and the id index are pure caches: deleting them changes no answer, only the cost of the next one. The rest — the chains, the pointers, the metadata files — are fufu's memory, and deleting them loses fufu conveniences, never repository content: undo's reach, a pending description, a parked entry's name, but every commit, branch, snapshot tree, and stash entry is ordinary git and stays reachable with ordinary git commands. And no record is ever authority over the repository: when a record disagrees with what the repository actually contains — a parked entry popped by hand, a branch moved by raw git — the repository wins, and reconciliation demotes the record and says so out loud. That rule and its consequences are [the invariant's](../concepts/invariant.md#a-cache-over-git-never-an-authority) strong form, and the whole layout is designed so abandoning fufu costs automation and returning to it is reconciliation, not recovery.
