# tower — design sketch

*Founding sketch, August 2026. Speculative: none of tower is built, though nearly everything it stands on is.*

**tower** is project management for people and agents, built on fufu. The crate is `ff-tower`, and it installs under two names: `ff-tower`, which is what fufu's `ff-<name>` dispatch finds for `ff tower`, and `tower`, so the verbs below can also be typed on their own.

The name comes from fufu's own metaphor. fufu is the pilot — it flies the repository. tower is the tower: it doesn't fly anything, it assigns work, sequences landings, and keeps traffic from colliding. Deconfliction is literally the job, and it is also the one thing a version-control-native tracker can do that no other tracker can.

## Thesis

Every tracker asks a human to say what is happening. Linear, Jira, GitHub Projects — all of them are a database of claims that someone remembered to update, drifting from the repository the moment attention lapses.

fufu already knows. Capture runs before every action, futures are computed for free, branches and sessions are observable. So:

> **State is derived from the repository, never entered. Only intent is stored.**

Stored: title, body, links, priority, assignee, dependencies — the things a human authored and nothing can infer. Derived: everything else. A flight is `active` because a branch exists with snapshots on it, not because anyone clicked.

The consequence worth the whole design: tower sees work start before the first commit exists, because the capture floor does. An agent that edits for twenty minutes and commits nothing is visible. No other tracker can see that, because no other tracker has a record of it.

## The seam

tower is a separate program. Issue tracking is not a version control operation, and fufu's principle 10 — verbs must earn their existence — kills it as a native verb on its own merits. It ships in the fufu workspace and release, discovered as `ff tower`, but with its own authority, its own store, and its own cadence.

The contract:

```
reads     ff status --json · ff log --json · ff collide --json · ff watch
calls     ff start · ff switch, every call tagged --session <flight>
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

Merge simulation is free and side-effect-less — the whole replay runs inside one object-memory clone and writes nothing — so tower can ask "would these two land on each other?" continuously, about work that has not been committed yet. `ff collide` is that question already spelled: one verdict per pair of branches with the paths where two of them touch the same thing, and the largest set that touches nothing. It reads each side's tree from the operation log rather than from a worktree, so a branch an agent is editing right now in another bay, with nothing committed, still answers. Tower does not need a probe of its own; it needs the verb's JSON and a reason to ask.

Two kinds of blocking, and they differ in kind:

- **declared** — a human said this depends on that. Stored intent. Every tracker has it.
- **discovered** — a merge probe found two branches inside the same hunk. Nobody typed it, it appeared the moment the second edit happened, and it disappears on its own when one lands.

From discovered conflicts comes a **land order**: topologically sort in-flight work by pairwise conflict, and say which sequence costs nothing. The clear set `ff collide` closes with is the same answer pointed at assignment rather than at landing, which is what `tower next -n <k>` hands out. And once bays make "what is in the air right now" queryable, the check moves to assignment time — tower holds back a flight that would collide with one already flying instead of filing an incident after the fact. Sequencing on approach, not collision reporting.

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

An agent that hits a real question holds the flight with the question attached: the bay stays warm, the capture chain is intact and still carries the flight's session tag, and nothing was guessed. Answering resumes it where it stopped — and because a session is a tag rather than something opened, there is no state a hold could leave dangling.

This is fufu's `held` verbatim — nothing was touched and a human decision is required, exit code 3 — and it inherits principle 8 with it: announced at creation, pinned until answered, exits blocked. An agent question that goes quiet is how the whole system rots.

Waiting is a state, not a process. Nothing resumes a held flight until someone asks; no daemon is required for any of it.

## Bays

Parallel agents need parallel working trees, and git worktrees are the idiom (principle 4). fufu's per-branch state lands on them almost by accident: snapshot chains, branch metadata, and futures caches are keyed by branch under the common dir, and a worktree is one branch, so that half collides over nothing.

**The operation log used to be the half that collided, and fufu has fixed it.** It was one ref for the whole repository — a single chain across every branch, one lock, and `ff undo` a pointer move on that one chain — so three agents in three bays shared one undo pointer and one queue. Sessions softened it and could not solve it: undo steps over a *run* of adjacent captures carrying the same tag, but adjacency is a fact about the log rather than about the bay, and interleaved agents do not produce adjacent runs. The chain is now keyed by worktree at `refs/fufu/wt/<id>/ops`, with its own undo pointer and its own lock, and a bay's ref table holds only the refs that bay owns. A bay's undo walks back its own steps and no one else's.

The cost that bites is bootstrap, not disk — everything gitignored (`target/`, `node_modules`, venvs, `.env`) does not come along, so per-flight creation means a cold build per flight. Hence a **pool of warm bays**, bootstrapped once and recycled, rather than create-and-destroy. A shared `CARGO_TARGET_DIR` is the tempting shortcut and a trap: cargo's file lock serializes the builds you bought concurrency to parallelize. The pool calls `ff worktree add` and `ff worktree remove` rather than shelling out to git: the chain floor then exists before the agent's first command, and a recycled bay's work is captured before the bay is torn down.

`bays: 1` must stay a supported configuration. Serialized agents in one tree lose throughput and keep every other feature, including deconfliction, and whether concurrency pays depends entirely on a project's cold-start cost.

Note that bays make fufu's tree memory moot for the agent lane — an agent owning a tree for the life of a flight never parks or switches. That is fine. Humans still switch, and Floor 2 still serves them.

Bays are not agent-only, though, and the `review` procedure is the case that proves it: its human part is *check out someone else's branch and run it*, which is a bay with a foreign branch in it. Filing a review can warm one, so the tree is already built by the time you sit down. That is the moment the pool pays for itself for a person rather than a fleet.

## Storage and sync

Not files in the working tree. `.tower/flights/*.md` is the obvious move and the trap every git-native tracker falls into: the board becomes branch-dependent, ticket edits pollute code diffs, and closing something on an unmerged branch means the board lies until merge.

**An orphan ref, shaped like fufu's journal.** `refs/tower/log/<author>` — a commit chain with its own tree, no relation to code history, never touching the working tree, CAS-appended, reachability as the gc pin. Sync is one explicit refspec.

The conflict problem dissolves because of what is stored. Derived fields are never stored at all, so they have zero merge surface and self-heal when someone works around tower. Stored intent is an append-only event log partitioned per author, so merging divergent logs is a **union, not a merge** — conflict-free by construction. The board is a fold over the union. The only genuine collision is two people editing one field in the same window; last-writer-wins with a stable tiebreak, and both events survive in the log regardless.

**Sync is three tiers, and only one of them needs anything built.** *Machine-local* — bays, pool state, caches — never syncs and mostly rebuilds. *Mine across machines* — solo flights, notes, decompositions — is single-author and append-only, so roaming is `git push refs/tower/log/<me>` with no protocol at all; that is backup, not sync. *Shared with others* is the only hard tier, and tower does not have it: in team mode upstream already holds it, and in solo mode it does not exist.

Multi-writer works anyway — fetch `refs/tower/log/*`, fold the union — and it stays documented and unsupported. Every git-native tracker that tried to be the shared board was technically fine and socially dead: shared work needs a place people look, and a ref in a repository is not one. Making it one means notifications, identity, and permissions, which is a different product wearing this one as a hat. **tower never becomes the shared board; sharing is `tower promote`.**

The deeper reason is that tower has no mechanism for agreement. Facts need no consensus — the branch exists, these hunks collide, CI failed — which is why tower can assert them unilaterally and be believed. Upstream state is negotiated: priority, ownership, what ships this cycle. A shared tower board would manufacture consensus data with nothing underneath it, and two people would confidently read different boards.

One honest consequence: this is the first fufu-adjacent state that is not a cache. fufu's principle 3 says state is rebuildable and the repository wins; authored text is derivable from nothing. It holds anyway — the store *is* ordinary git objects in the repository, so the repository still wins literally — but authored flights are losable in a way no fufu state is. That earns the held-rewrite treatment: `14 flights exist only on this machine · tower push`, pinned on every render until it is false, with a doctor row beside it.

## Surfaces

One model, every renderer — fufu's principle 14, so the MCP server is a thin shell over the same contract the CLI renders, never a second implementation.

```
caller          surface        what it does
────────────────────────────────────────────────────────────────
a person        CLI            decide, answer, route, publish
an agent        MCP            claim, read a brief, report, hold
nothing         —              no daemon, no cron, no spawns
```

The board is an inbox, in four sections matching four states of mind: **waiting on you** (agent questions, review requests, changes requested), **in the air** (bays, with live conflict verdicts), **holding** (CI, merge queue, blocked on a person), **open**.

Inside *waiting on you*, partition by what the item costs you rather than by priority — an answer is thirty seconds, a review is twenty minutes, a decision is unbounded. Sorting them together means you cannot spend the five minutes you actually have. Cost is read off the shape of the thing, so it needs no judgment and no model call.

The whole design is aimed at one reflex: bare `tower`, often, because it is the fastest way to learn what to do next. Two things have to hold or the reflex never forms. It has to be true, which is the derived-not-entered thesis. And **render must never block on the network** — fold the local log, draw, note the age, refresh on the cadence stamp. A board that is fresh and slow loses to one that is instant and honest about how stale it is.

The review loop deserves modeling directly, because it is mostly waiting and mostly agent-shaped: an incoming review is work arriving, and sorting its comments into what a machine can carry out and what needs a decision is where the ergonomic win lives. Answer the one design question, let the other three land.

### The verbs

fufu's rule that every verb must earn its existence carries over, and the one it kills first is `run`. Tower cannot run anything — a verb that implies dispatch would be the first crack in principle 2, and that line is too load-bearing to contradict casually. Starting work under a procedure is `tower file`, because filing is what actually happens; the decomposition and the first brief fall out of it.

| verb | what it does | caller |
|---|---|---|
| `tower` (alias `board`) | the inbox: what needs you, what is in the air, what is holding, what is open | you |
| `tower next [-n <k>]` | claim the next ready flight, or a set of `k` that collide with neither each other nor anything already flying; `--peek` reads without claiming | an agent |
| `tower claim <flight>` | claim one specific flight, out of order | either |
| `tower file <procedure> [<subject>]` | put work on the board under a procedure — the one front door, adapter or hallway | either |
| `tower triage` | walk the unclassified pile and route each item to a procedure | you |
| `tower take <flight>` | take the controls: crew this to you, agent off | you |
| `tower requeue <flight>` | the reverse — hand it back to the pool | either |
| `tower brief <flight>` | everything known about this flight: subject, files, prior art, verify command, handoff notes | an agent |
| `tower hold <flight> -m <question>` | stop with a question attached — bay warm, session open, exit 3 | an agent |
| `tower answer <flight> -m <answer>` | answer it and release the hold | you |
| `tower done [<flight>]` | finish a part whose completion nothing can derive; a smoke test that went fine leaves no trace | you |
| `tower link <a> <b>` | declare that one flight depends on another — discovered conflicts need no verb | either |
| `tower comment <flight> -m <note>` | a note on the record, local; saying it to the team is a separate, deliberate gesture | either |
| `tower decompose <flight>` | file a procedure's parts, or split further by hand | either |
| `tower promote <flight>` | mint the upstream ticket, link it, keep local history — the publish boundary | you |
| `tower bay <list\|warm\|release>` | the pool: what is bootstrapped, what is occupied, what to build ahead of you | either |
| `tower explain <flight>` | why this is here, why this procedure, and what it beat | you |
| `tower procedures [<name>]` | what is installed, what each matches, and where to fork it | you |
| `tower push` | push your log ref — backup and roaming, the one outward gesture the team never sees | you |
| `tower config` | settings, on fufu's typed-registry model | you |
| `tower doctor` | unpushed flights, stale adapters, bays that no longer resolve | you |
| `tower <adapter> <args>` | passthrough to `tower-<adapter>` on PATH: `tower linear`, `tower github` | either |

Every one of them is a read plus a local write. Nothing in the column on the right is a dispatch target.

## Triage

Triage asks two questions, and the ordering one is second. First is *what is this and therefore what happens to it*, which is the next section. This one is the ranking that follows, and the two are orthogonal — a review flight can be waiting on you or in the air like any other.

Triage splits on the same line everything else does. Blocked or not — by a declared dependency, a discovered conflict, or a person who has not replied — is a graph query, a merge probe, and an upstream read. Cost to start is a warm bay, an existing branch, and which files this week's capture chain touched. What changed since you last looked is a log diff. All of it is computation; none of it is judgment.

The algorithmic half is most of the value, because the hard part was never ranking. Filtering out everything that cannot be started right now routinely takes thirty items to four, and at four the ordering barely matters. The good-enough algorithm is good enough precisely because it declines to judge importance: filter on readiness, partition into the four sections, sort by upstream priority then readiness then age, and leave importance to whoever set the priority field. A tracker that does not invent its own opinion about what matters is more trustworthy, not less.

Then **explain the pick and what it beat**. That line is load-bearing: an explained ranking is correctable in one glance, an unexplained one is a black box you stop trusting after two bad calls — which is the failure mode that would sink the whole product.

Genuine judgment stays out. Whether a review comment is mechanical or a decision, whether two flights are the same, whether a body is too vague to hand off, how to decompose a goal — none of it is tower's. tower attaches *facts* to a comment: resolved, still on a live line, carries a candidate patch. It never attaches a verdict. A suggestion block being syntactically applicable says nothing about whether it should be applied — the reviewer can be wrong, and reviewing the review is an agent's job or a person's.

Which forces one rule, or the board stops being trustworthy:

> **An agent's triage output is stored as intent, never recomputed as state.**

A model call at render time makes the board flicker: same data, different call, different answer. Judgments are frozen into the log, attributed to the agent that made them, overridable, and never re-run behind your back, so the board stays a pure function of (repository, log). That is not a compromise of derived-not-entered — agent judgment *is* entered, merely entered by an agent.

Errors are asymmetric, so defaults lean conservative. Filing a decision as mechanical is expensive: something quietly makes a design call nobody reviewed. Filing mechanical as needs-you costs one extra line of reading. Everything ambiguous goes to needs-you.

Three things tower should not build: **estimates** (measurable for started work, fiction for unstarted — report what is known and invent nothing), **learned ranking** (no data on day one and not enough for a long time; weights live in config and are tuned by hand), and **automatic deduplication** (semantic, rarely urgent, expensive when wrong).

## Procedures

Work does not arrive in one shape. A ticket assigned to you, a review requested of you, a thing your manager asked for in a meeting — each has a different first step, a different split between what a machine can carry and what only you can, and a different meaning of done. Ranking a homogeneous list has nothing to say about which list.

A **procedure** is a named recipe for one shape of work: what it decomposes into, who flies each part, and when it is finished. A signal is classified into one at intake, and everything downstream — board section, brief, claimability, completion — follows from that stamp. The word is the metaphor's: a published procedure is a standard sequence for a recurring situation, and the tower clears you for one by name. A plan and a permission, never a hand on the yoke.

The shipped set is small, because the point is that people fork it:

| procedure | the signal | parts, in order |
|---|---|---|
| `ticket` | assigned work, whether or not it exists upstream yet | research · **promote** · implement · **review** |
| `review` | someone else's work you have been asked to look at | agent pass and **smoke test**, concurrently · **verdict** |
| `open` | anything unclassified | one part, **yours** |

Bold parts are crewed to you. Almost nothing here is a new primitive: a procedure is a decomposition template plus a crew assignment per part, riding on `file`, `link`, and `brief`. That the `ticket` procedure contains its own promotion is the entire research-first workflow — a flight exists before its upstream identity, research produces the body, `tower promote` mints the ticket. That is local-steps-are-anonymous-branches walked one step forward, with principle 3 putting your hand on the promotion because it is the moment the team sees anything.

**Every procedure ends with you.** Not a default — principle 3 restated at the flight level. The boundary where the team sees the work is always a human gesture, so the last part is always yours. A procedure with no human part is not a procedure, it is a script.

**Procedures declare structure; skills hold judgment.** A procedure is data — a name, match rules, parts, crew, a done condition — and it cannot express control flow. No conditions, no loops. Everything conditional lives in the skill an agent-crewed part points at, in markdown, which is where this document already puts judgment. The moment a procedure needs an `if`, it is a skill. That rule is the only thing between this feature and Jira's workflow editor, which is where configurable trackers go to die: the config language grows into a bad programming language and the shipped defaults become nothing.

The same test draws every part boundary: a part ends where the crew changes or a gate stands, and nowhere else. Two agent-crewed stretches with nothing between them are one part, and the sequencing inside is the skill's business — which is why `ticket` researches and drafts the body in a single part rather than two.

### Shape

Two files. The structure is data, the judgment beside it is markdown, and the split is principle 13 made physical. Definitions layer the usual way — yours roams with your config, the repository's is the team's, merged by name with the more specific winning. This is not the working-tree trap from *Storage and sync*: what must never live in the tree is derived, mutable board state, and a procedure definition is config that changes monthly.

```toml
name    = "review"
subject = "branch"            # may resolve to a PR later

[[match]]                     # only ever runs on adapter signals
source = "github"
event  = "review_requested"

[[part]]
id    = "pass"
crew  = "agent"
skill = "review"
done  = "asserted"

[[part]]
id   = "smoke"
crew = "you"
bay  = "warm"                 # build the tree ahead of me
done = "asserted"

[[part]]
id    = "verdict"
crew  = "you"
after = ["pass", "smoke"]
done  = "asserted"
```

Order is a DAG through `after` — the same edges `tower link` writes — so concurrency is the absence of a declaration rather than a keyword: `pass` and `smoke` fly together because neither names the other.

**`done` is a closed enum**: `asserted` (the crew says so), `committed`, `promoted`, `landed`. Four values cannot grow into an expression language, which is the whole point. *Done when CI is green and two people approved* is a human-crewed part you assert, and what convinces you belongs to the skill.

**The definition is read once, at file time, and its parts are copied into the log.** Filing writes the flight: the procedure stamp and which rule matched, the subject and its resolution state, and one instance per part carrying crew, skill, edges, and claimant. Readiness, conflicts, order, and section stay derived as ever. Editing a procedure therefore never disturbs a flight already in the air — a board that re-read config at render time would flicker for exactly the reason principle 11 forbids re-running judgment, and forking a procedure mid-week has to be safe or nobody will.

### Intake

**Every signal comes through one front door.** A GitHub review request, a Linear assignment, and a hallway conversation are one event with different provenance, and `tower file` is the same intake path an adapter takes. If the human-originated signal is second class, a large fraction of most people's week is invisible and the board lies about the day.

Intake is a read, not a subscription — upstream is pulled lazily at invocation, as everything else here is. So it does not matter where work was born: file the ticket by hand in Linear, and the next call picks it up with no webhook and nothing running in between.

Classification is deterministic and stored, never recomputed; principle 11 governs routing exactly as it governs triage. Rules match on facts an adapter or a person supplied, run once when the signal lands, and leave an overridable event in the log. **The routing is explained** for the same reason the ranking is: *classified `review` because upstream sent `review_requested`* is correctable in a glance, and a silent stamp is a black box you stop trusting on the second bad call.

Ambiguity goes to you and stays unclaimable. Asymmetric errors again, with teeth this time: an orchestrator looping on `tower next` will otherwise eventually claim a vague meeting request and start editing files. `next` returns only flights whose procedure declares an agent-crewed first part, unclassified work sits in your lane, and `tower triage` is the walk through that pile.

**A flight's subject resolves late.** File a review against a bare branch with no PR, or a ticket that exists nowhere — tower holds a local subject, derives what the repository shows, and stays silent about fields it cannot see. When the PR opens or the ticket is minted, the adapter links it and upstream truth flows into the fields it owns. Both shipped procedures need this, and it is one mechanism rather than two special cases.

Which forces one piece of exactness: a signal arriving for a subject you already filed merges into that flight as a `foreign` event rather than filing a second one. This is identity equality on a resolved reference, cheap and exact, and deliberately not the semantic deduplication this document declines to build. Without it, being faster than your own sync double-files every review you noticed before the forge told you about it.

## Skills

tower ships agent skills, and they are where the orchestrator lives. That is not a contradiction of principle 2: a skill is instructions the harness executes, not a process tower spawns. tower ships the recipe, the harness runs it, and uninstalling the harness leaves tower working. tower never grows a process supervisor.

It is also the right home for judgment. tower reports facts and what is clear; a skill decides what to do when a flight holds, when a review comment needs a person, when to stop. Policy in markdown the user can fork beats policy compiled into Rust.

The shipped set is small: **plan** (decompose a goal into linked flights — solo mode's entry point), **work** (claim, do, hold or commit, repeat — the one that pairs with a loop), and **review** (first-pass a review request, or apply the mechanically-fixable half of one and hold the rest). Each agent-crewed part of a procedure names the skill it is flown with, which is the seam that keeps structure in data and judgment in prose.

Loop control is exit codes, fufu's own: **0** here is work, **1** nothing available, **3** work exists but it needs you. A loop runs until 1 or 3 and reports which. No timeout, no sentinel.

Fan-out needs a set, not an item, because conflict-freedom is a property of the set: `tower next -n 3` returns three flights that collide with neither each other nor anything already flying, and the caller spawns one agent per bay. That is deconfliction as an API rather than a report, and it is the sharpest reason the design is worth building. The set itself is `ff collide`'s clear set, filtered to what is claimable; tower's contribution is the filter and the claim, not the verdict.

The shipped default stops short of the push boundary — committed on a branch, PR unopened — because principle 3 is easy to state and easy for an unattended loop to violate fourteen times before anyone looks. Editing that is the user's call, and visibly theirs.

## The three modes

**Solo** — no adapters. An agent decomposes a goal, calls `tower.file` per step and `tower.link` for the order, and tower stores a DAG it did not author. Then context can be wiped safely, because tower is the durable half: the plan, each brief, the handoff notes, and every capture chain live outside the agent. The agent is disposable; the flight is not.

**Team** — adapters installed. Upstream owns its fields, tower owns the local layer, and the local layer is where the actual day happens.

**In between** — one upstream ticket, many local steps, one promotion when a step outgrows the local board.

Three layers of memory stay apart: a **skill** knows how to drive tower, the **agent's own memory** knows house style and conventions, and a **brief** knows this flight — files, prior art, the verify command. tower owns only the third. A skill that starts accumulating project conventions has taken the agent's job, and tower trying to own house style would do it badly when the agent already has a system for it.

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
10. **Facts, not consensus.** tower is authoritative over what the repository shows and what you alone authored. It holds no negotiated state, because it has no way to negotiate.
11. **Judgment is stored, never recomputed.** A model's verdict is written to the log as authored intent. The board stays a pure function of repository and log, or it flickers and is not believed.
12. **Every procedure ends with you.** Principle 3 at the flight level: the last part of any shape of work is human-crewed, because the boundary where the team sees it always is.
13. **Procedures declare structure; skills hold judgment.** Procedures are data and carry no control flow. Every conditional lives in markdown a person can fork.

## What it stands on

Four fufu surfaces carry most of this, and all four exist.

- **`ff collide`** is the sideways axis. Base and remote were never the interesting pair for a tracker; every discovered conflict, land order, and assignment-time holdback is sibling against sibling, and that is the axis this verb points. It reports per pair and closes with the largest clear set, which is `tower next -n <k>` waiting for a caller.
- **`ff watch`** streams the operation log as newline-delimited JSON, and `--session <name>` narrows it to one tag — so a flight's own motion is a subscription rather than a poll. It reports what the log *did* rather than what was appended: an undo that steps the pointer back, a fork after one, a trim that rewrites every id a subscriber holds. Tower must handle those the way any subscriber does, because the board's ids are the log's ids wherever a flight points at capture. `--all` is the fleet form: every chain in the repository on one stream, with a `worktree` field on every line — the field the board keys on, present in both modes — so `bays: N` is one process rather than N. Bays that appear mid-stream join it, retired bays keep their place through their last capture, and a trim in one bay ends that bay's addresses rather than the stream.
- **`ff publish`** is the outgoing half, and it is why `review` and `landed` are derivable at all: `ff sync` takes in, `ff publish` sends, and only the second one leaves the machine.
- **Sessions** are a tag on an operation and nothing more. `--session <name>` rides every fufu command, lands as a `fufu-session` trailer, and serves as the equality test that groups adjacent captures into one `ff undo` step. There is nothing to open or close: every fufu call tower makes carries `--session <flight>`, per-flight capture chains fall out of the tagging, and the extension seam hands `FF_SESSION` down to a child process, so an adapter's own `ff` calls inherit the tag without re-passing the flag.

## What it waits on

Load-bearing and absent:

- **~~One operation log across many bays.~~** *Answered.* fufu keys the chain by worktree, so each bay has its own log, its own undo pointer and its own lock, and records only the refs it owns. This was the largest thing tower waited on, and `bays: N` no longer waits on it. The reading half landed with it: `ff watch --all` is one stream over every chain in the repository, each line naming the worktree it came from, so a supervisor over a pool subscribes once instead of per bay. What remains is tower's own.
- **Forge reads.** The `review` procedure stands almost entirely on state the repository cannot see, so the adapter that supplies it is a dependency of a shipped default rather than a nicety. This one is tower's own to build.
- **~~A handshake at the extension seam.~~** *Answered.* `ff <name>` hands a child `FF_REPO` — the worktree it was invoked against, absolute and resolved, unset outside one — alongside `FF_CONTRACT` and the session tag. A tower adapter reads which repository it is in rather than rediscovering it, and reads the envelope version before parsing an envelope. `ff -C <dir>` landed with it, so a bay is addressable without spawning from its directory: one process can ask every bay in the pool.

What works today is most of it: the board through `active`, flight-to-branch linkage, the event log store, per-flight session tags, briefs, holds, land order, and the conflict-free set `next -n <k>` hands out. The deconfliction that is the reason to build tower is available now. What is missing is the concurrency to spend it on, and the forge state one of the two shipped procedures reads.

## Open questions

- **Triage quality is the product.** The deterministic half is settled above, and it covers *waiting on you* — the section that has to be right. What stays open is the weighting inside `open`, which has no data behind it on day one. Argues for shipping a read-only board against real upstream data long before anything is allowed to claim work.
- **Does the flight own the branch, or the branch own the flight?** If `ff branch <name>` claims a placeholder, does claiming mint a flight? The everything-is-a-flight version is seductive and probably wrong.
- **How much forge state to absorb.** Not whether — the `review` procedure settles that — but where it stops. Every field pulled in punctures the derived-from-the-repo purity a little further, and the ownership table is the only thing keeping that from becoming a second tracker.
- **Whether the `done` enum stays at four.** It is closed on purpose, and the first genuinely missing value is the moment to check whether the answer is a fifth constant or a part nobody wanted to crew.
- **What a flight means after a rewrite** folds its snapshots into a commit — fufu's open session-boundary question, made urgent rather than theoretical.
- **Bay relocation.** tower prints a path and cannot make a running agent honor it. How loudly should misplaced work be reported, and is there a consented way to move an agent?
- **Sandboxing composes but is unaddressed.** A bay can be a worktree bind-mounted into a container without tower's model changing; whether that is tower's concern at all is open.
- **What loop control is on MCP.** The exit codes are fufu's and they are right for a shell loop, but MCP returns a result and has no exit code to carry 0/1/3. Either the three states become a field the tool returns and the exit codes are the CLI's rendering of it, or the agent lane loops through the CLI and MCP is for reading. Principle 9 says one model, so the answer is probably the first, and it is not decided.
- **How much orchestration belongs in a shipped skill** before it is a scheduler with extra steps and principle 2 has been defeated by paperwork.
- **Naming.** `tower` against crates.io, npm, and Homebrew. Almost certainly taken; the metaphor is what matters, not the word.
