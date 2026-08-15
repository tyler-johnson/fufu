# tower — design sketch

*Founding sketch, August 2026. Speculative: much of what this depends on isn't built.*

**tower** is project management for people and agents, built on fufu. The crate is `ff-tower`; the binary is `tower`, and it registers as `ff work` through fufu's `ff-<name>` extension mechanism.

The name comes from fufu's own metaphor. fufu is the pilot — it flies the repository. tower is the tower: it doesn't fly anything, it assigns work, sequences landings, and keeps traffic from colliding. Deconfliction is literally the job, and it is also the one thing a version-control-native tracker can do that no other tracker can.

## Thesis

Every tracker asks a human to say what is happening. Linear, Jira, GitHub Projects — all of them are a database of claims that someone remembered to update, drifting from the repository the moment attention lapses.

fufu already knows. Capture runs before every action, futures are computed for free, branches and sessions are observable. So:

> **State is derived from the repository, never entered. Only intent is stored.**

Stored: title, body, links, priority, assignee, dependencies — the things a human authored and nothing can infer. Derived: everything else. A flight is `active` because a branch exists with snapshots on it, not because anyone clicked.

The consequence worth the whole design: tower sees work start before the first commit exists, because the capture floor does. An agent that edits for twenty minutes and commits nothing is visible. No other tracker can see that, because no other tracker has a record of it.

## The seam

tower is a separate program. Issue tracking is not a version control operation, and fufu's principle 10 — verbs must earn their existence — kills it as a native verb on its own merits. It ships in the fufu workspace and release, discovered as `ff work`, but with its own authority, its own store, and its own cadence.

The contract:

```
reads     ff status --json · ff log --json · ff watch (ndjson) · futures
calls     ff start · ff switch · ff session start/end
stores    refs/tower/log/<author>
derives   state · progress · conflicts · land order
writes    nothing under refs/fufu/*, ever
```

That last line is fufu's extension rule, unmodified: extensions read fufu state and call fufu verbs; only fufu writes fufu state.

## Passive by construction

**tower is a thing agents call. It never calls agents.**

Every verb is a read plus a local write. There is no daemon, no cron, no dispatch, no iteration verb. If work should loop, the agent harness loops and calls `tower next` again — the harness is the scheduler, tower is only the queue.

The reasons compound. Initiating means owning agent lifecycle: keys, model selection, retries, context limits, per-vendor quirks — a second product, and a moving one. Staying passive makes tower vendor-neutral by construction, because it never learns who is calling. And a queue that dispatches on its own is a background process making outward-facing decisions nobody asked for that minute, which fufu's principle 9 already forbids in its own domain.

Identity is a caller fact, not a dispatch target: `--as qwen` means qwen is calling, never send this to qwen.

Sync follows the same discipline. Upstream is pulled lazily at invocation, gated by a cadence stamp, the way fufu's auto-trim and update check already work. The board is fresh because you just asked for it. Anything that needs to reach you unasked belongs in fufu's ambient shell channel — a heartbeat the user started — not in a process tower spawned.

Tower cannot enforce, only observe and complain. `tower next` prints the bay path; it cannot relocate a running agent and does not try. Work landing on the wrong branch is reported loudly at the next render rather than prevented by a hook. That is fufu's regime boundary, inherited.

## Deconfliction — the earned existence

`merge-tree` is free and side-effect-less, so tower can ask "would these two land on each other?" continuously, about work that has not been committed yet.

Two kinds of blocking, and they differ in kind:

- **declared** — a human said this depends on that. Stored intent. Every tracker has it.
- **discovered** — merge-tree found two branches inside the same hunk. Nobody typed it, it appeared the moment the second edit happened, and it disappears on its own when one lands.

From discovered conflicts comes a **land order**: topologically sort in-flight work by pairwise conflict, and say which sequence costs nothing. And once bays make "what is in the air right now" queryable, the check moves to assignment time — tower holds back a flight that would collide with one already flying instead of filing an incident after the fact. Sequencing on approach, not collision reporting.

This is fufu's principle 7 raised one level: if an outcome can be known in memory for free, the board should already know it.

## Upstream is a foreign writer

At work the team already has a tracker. tower does not replace it and is never authoritative over it. This is fufu's principle 2 one layer up: Linear and GitHub are first-class foreign writers, observed and absorbed, never owned.

Field ownership is enforced hard, or sync becomes a merge problem it does not need to be:

| owner | fields | status |
|---|---|---|
| upstream tracker | exists, title, body, assignee, priority, cycle | upstream truth |
| forge | PR, review state, CI, merge | upstream truth |
| the repository | branch, snapshots, session, conflicts, order | derived by fufu |
| tower | queue, bays, claims, local steps, briefs, notes | local truth |

Upstream changes arrive as `foreign` events in the local log — labeled, undoable, loud — and upstream wins every field it owns. tower holds a pointer and a local layer beside it; it never merges into someone else's model.

**Never auto-outward.** Automation moves local state freely: claim, brief, bay, decompose, requeue. Anything the team sees — opening a PR, posting a comment, moving an upstream status — is a deliberate gesture. An agent commenting at machine rate is a social failure with no technical apology.

Adapters are the same fractal: `tower linear` runs `tower-linear` from PATH. Solo mode is the case where none are installed, and nothing else changes.

## Local steps are anonymous branches

A team ticket decomposes into steps that are real, tracked, briefed, and assignable — and invisible upstream. They are fufu's anonymous branches: genuine from birth, merely not yet named to anyone outside.

Promotion is the same gesture at the same boundary. A step that turns out to need a teammate or a PR of its own gets `tower promote`, which mints a real upstream ticket, links it, and keeps the local history — exactly `ff branch <name>` claiming a placeholder at the publish boundary.

The team's board stays as coarse as the team wants. The local board is as fine as the work actually is. Neither has to negotiate with the other.

## Held

An agent that hits a real question holds the flight with the question attached: the bay stays warm, the session stays open, the capture chain is intact, and nothing was guessed. Answering resumes it where it stopped.

This is fufu's `held` verbatim — nothing was touched and a human decision is required, exit code 3 — and it inherits principle 8 with it: announced at creation, pinned until answered, exits blocked. An agent question that goes quiet is how the whole system rots.

Waiting is a state, not a process. Nothing resumes a held flight until someone asks; no daemon is required for any of it.

## Bays

Parallel agents need parallel working trees, and git worktrees are the idiom (principle 4). fufu's per-branch state lands on them almost by accident: chains, ids, and branch metadata are keyed by branch under the common dir, and a worktree is one branch, so N bays collide over nothing.

The cost that bites is bootstrap, not disk — everything gitignored (`target/`, `node_modules`, venvs, `.env`) does not come along, so per-flight creation means a cold build per flight. Hence a **pool of warm bays**, bootstrapped once and recycled, rather than create-and-destroy. A shared `CARGO_TARGET_DIR` is the tempting shortcut and a trap: cargo's file lock serializes the builds you bought concurrency to parallelize.

`bays: 1` must stay a supported configuration. Serialized agents in one tree lose throughput and keep every other feature, including deconfliction, and whether concurrency pays depends entirely on a project's cold-start cost.

Note that bays make fufu's tree memory moot for the agent lane — an agent owning a tree for the life of a flight never parks or switches. That is fine. Humans still switch, and Floor 2 still serves them.

## Storage and sync

Not files in the working tree. `.tower/flights/*.md` is the obvious move and the trap every git-native tracker falls into: the board becomes branch-dependent, ticket edits pollute code diffs, and closing something on an unmerged branch means the board lies until merge.

**An orphan ref, shaped like fufu's journal.** `refs/tower/log/<author>` — a commit chain with its own tree, no relation to code history, never touching the working tree, CAS-appended, reachability as the gc pin. Sync is one explicit refspec.

The conflict problem dissolves because of what is stored. Derived fields are never stored at all, so they have zero merge surface and self-heal when someone works around tower. Stored intent is an append-only event log partitioned per author, so merging divergent logs is a **union, not a merge** — conflict-free by construction. The board is a fold over the union. The only genuine collision is two people editing one field in the same window; last-writer-wins with a stable tiebreak, and both events survive in the log regardless.

One honest consequence: this is the first fufu-adjacent state that is not a cache. fufu's principle 3 says state is rebuildable and the repository wins; authored text is derivable from nothing. It holds anyway — the store *is* ordinary git objects in the repository, so the repository still wins literally — but unpushed flights are losable in a way no fufu state is, and doctor needs a row saying so.

## Surfaces

One model, every renderer — fufu's principle 14, so the MCP server is a thin shell over the same contract the CLI renders, never a second implementation.

```
caller          surface        what it does
────────────────────────────────────────────────────────────────
a person        CLI            board · answer · promote · file · triage
an agent        MCP            next · brief · hold · file · link · comment
nothing         —              no daemon, no cron, no spawns
```

The board is an inbox, in four sections matching four states of mind: **waiting on you** (agent questions, review requests, changes requested), **in the air** (bays, with live conflict verdicts), **holding** (CI, merge queue, blocked on a person), **open**.

Triage is the whole product. `tower next` beats an assigned-to-me list because it knows the cost *right now* — which bay is warm, what collides with what is flying, which files were touched this week, whether the blocker is a person or a build. A P1 that conflicts with an occupied bay is worse work this minute than a P3 that is clear.

The review loop deserves modeling directly, because it is mostly waiting and mostly agent-shaped: an incoming review is work arriving, and triaging its comments into mechanically-fixable versus needs-a-decision is where the ergonomic win lives. Answer the one design question, let the other three land.

## The three modes

**Solo** — no adapters. An agent decomposes a goal, calls `tower.file` per step and `tower.link` for the order, and tower stores a DAG it did not author. Then context can be wiped safely, because tower is the durable half: the plan, each brief, the handoff notes, and every capture chain live outside the agent. The agent is disposable; the flight is not.

**Team** — adapters installed. Upstream owns its fields, tower owns the local layer, and the local layer is where the actual day happens.

**In between** — one upstream ticket, many local steps, one promotion when a step outgrows the local board.

Two kinds of memory stay apart: a tower brief carries what is true about *this flight* — files, prior art, the verify command. The agent's own memory carries house style and conventions. tower should not try to own the second; it would do it badly, and the agent already has a system for it.

## Principles

1. **Derived, not entered.** State comes from the repository. Only authored intent is stored.
2. **tower is called; it never calls.** No daemon, no dispatch, no loop. The harness schedules; tower queues.
3. **Never auto-outward.** Local state moves freely; anything the team sees is a deliberate gesture.
4. **Upstream owns its fields.** tower is never authoritative over someone else's tracker, and never merges into their model.
5. **Observe and complain, never enforce.** tower prints the path, reports the drift, and does not hook or veto.
6. **Conflict-free by construction.** Union-merged event logs, not a synced database.
7. **Local work stays local until promoted.** Steps are anonymous branches; promotion is the publish boundary.
8. **Deferred requires loud.** Inherited whole from fufu: a held flight is announced, pinned, and blocks its exits.
9. **One model, every surface.** CLI, MCP, and anything later consume one contract.

## What it waits on

Load-bearing and absent from fufu today:

- **`ff session`** — no `session.rs` in the tree. Briefs, work logs, handoff, and per-flight capture chains all sit on it.
- **Futures (Phase 3)** — every discovered conflict, land order, and assignment-time holdback.
- **`ff watch` (Phase 2 journal follow)** — the live board and continuous conflict re-checking.
- **`ff push` (Phase 5)** — `review` and `landed` are the two states tower cannot honestly derive without it.

What works on today's primitives: the board through `active`, flight-to-branch linkage, the event log store, `ff start <flight>`, briefs, and holds. That is a real v0 and it is already more automatic than a normal tracker — it just cannot do the deconfliction that is the reason to build it.

## Open questions

- **Triage quality is the product.** If *waiting on you* cries wolf twice a week, everything above is decoration. Argues for shipping a read-only board against real upstream data long before anything is allowed to claim work.
- **Does the flight own the branch, or the branch own the flight?** If `ff branch <name>` claims a placeholder, does claiming mint a flight? The everything-is-a-flight version is seductive and probably wrong.
- **PR state is forge truth, not repository truth.** Pulling it in punctures the derived-from-the-repo purity; the alternative is a board that cannot say `review`.
- **What a flight means after a rewrite** folds its snapshots into a commit — fufu's open session-boundary question, made urgent rather than theoretical.
- **Bay relocation.** tower prints a path and cannot make a running agent honor it. How loudly should misplaced work be reported, and is there a consented way to move an agent?
- **Sandboxing composes but is unaddressed.** A bay can be a worktree bind-mounted into a container without tower's model changing; whether that is tower's concern at all is open.
- **Naming.** `tower` against crates.io, npm, and Homebrew. Almost certainly taken; the metaphor is what matters, not the word.
