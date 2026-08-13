# fufu — design document

*Founding draft, August 2026.*

**fufu** (`ff`) is a version control interface built on the belief that jj got the
workflow right and git got the repository right — and that you can have both.

The name: jj is short for Jujutsu, the martial art of redirecting force instead of
opposing it. fufu answers from the same dojo — "fu" is the martial-arts syllable that
hacker culture borrowed for tool mastery (git-fu, shell-fu, commandlinefu); fufu is
your fu, doubled. In Japanese, fūfu (夫婦) is a married couple: two who operate as
one, which is the architecture — fufu and git, one household. It is also a West
African dish of starch pounded until smooth, which is roughly what fufu does to git.
The binary is `ff`: the left hand's mirror of `jj`, a double-tap on the index
finger's home key.

## Thesis

jj demonstrates a better daily workflow than git's:

- All work is automatically saved, always. There is no dirty state, no stash, no
  lost file.
- You can switch between lines of work at will and everything is simply ready.
- Editing any commit in a stack automatically rebases its descendants.
- Every operation is undoable.

But jj achieves this by being a **new VCS that treats git as a storage backend**.
Its own store is authoritative; the git-visible repo is a projection of it. That one
architectural decision is the source of everything uncomfortable about jj for a
git-fluent user: detached HEADs, branches that don't move as you work, commits with
machine-generated `.jjconflict-*` trees, git commands demoted to second class.

None of the workflow benefits *require* that decision. fufu inverts it:

> **git remains the VCS. fufu is the pilot.**

fufu is a primary daily interface layered on an ordinary git repository. It owns the
ephemeral and the automatic — capture, movement, history rewriting, undo — and
leaves the durable graph entirely to git.

Be clear about what adopting fufu means: it is partly a **workflow shift**, not a
transparent overlay. Using fufu is accepting a set of positions — your branches
rebase onto main rather than merging it in; unpublished commits are malleable by
default; force-pushing your own branches (leased, guarded) is routine, not
exceptional. jog changes nothing about how you work; fufu deliberately does. The
invariant below promises *compatibility* — the repository stays legible to every
tool and teammate — never *neutrality*. The opinions stop at the push boundary:
published history is append-only, and how work lands (merge commit, squash,
rebase) remains the team's and the forge's business, not fufu's.

## The invariant

**At every instant, the repository is a boring git repository.**

HEAD attached to a branch. Ordinary commits. `git status` legible. Collaborators,
CI, IDEs, and plain-git tooling see nothing unusual, ever. fufu never creates a
state that plain git cannot represent; it only automates the transitions between
such states.

Corollary: deleting fufu loses convenience, never data or comprehension. A user
stripped of fufu is slower, not stranded.

The corollary's strong form: fufu is **abandonable and returnable at any moment**,
not merely removable once. A GUI session, a teammate's raw git, a weekend on a
machine without fufu — all legitimate, all absorbable. This forces one deep design
rule: fufu's state (journal, rewrite map, parked trees, held rewrites) is a *cache
over git, never an authority*. When fufu's records disagree with the repository,
the repository wins and fufu rebuilds its picture from what it observes. Returning
is reconciliation, not recovery — and reconciliation is loud about anything it
remembered that reality no longer matches.

This invariant is what jj cannot offer (its conflicted commits and colocated-repo
projections are states plain git can't comprehend), and it is fufu's one
non-negotiable. Every design question below is settled by asking what preserves it.

## The two regimes

fufu's guarantees follow its surface. Inside it, jj's rules apply; outside it,
git's rules apply — exactly.

**Through fufu**, nothing interrupts you: switching engages tree memory, syncing
holds its conflicts for your schedule, every operation is one `undo` away. The
user who stays on the fufu surface never meets a conflict at a moment they didn't
choose.

**Around fufu** — a GUI switch, a raw `git pull`, an IDE's commit button — the
user gets git's exact documented behavior, including git's conflicts at git's
usual moments. That is expected, and it belongs to the user; fufu does not reach
into operations it didn't perform. What it does instead: capture around them,
absorb them into the timeline, and reconcile loudly afterward. Guards obey the
same boundary — `ff push` refuses a stack with held rewrites, while raw
`git push` is git, with the status channel getting loud after the fact rather
than a hook getting in the way.

GUIs and IDEs are therefore first-class writers, not tolerated exceptions. Every
git GUI keeps working identically — showing status, making commits, switching
branches — because fufu's conveniences accrue per-operation to whoever goes
through fufu, and cost nothing to whoever doesn't.

The boundary is execution path, not spelling: the recommended shell alias
(`alias git='ff git'`) moves *typed* git onto the fufu surface, where daily
forms translate into fufu verbs (see `ff git`, below) — while everything that
resolves git on PATH stays foreign.

## Architecture: three floors

### Floor 1 — Capture (the foundation)

Every working-tree state is continuously snapshotted into refs outside the visible
graph — before commands, at prompts, around any mutation. jj's "the working copy is
a commit" becomes "the working copy *casts* a commit": same guarantee (no state ever
exists only in the filesystem), but HEAD never moves anywhere strange.

Once nothing can be lost, every other layer is allowed to be aggressive. Automatic
history rewriting is only a defensible feature on top of total capture.

This floor is proven technology: it is what jog does today. jog is the proving
ground for fufu, not its ancestor — lessons carry over, compatibility is not owed.

### Floor 2 — Time

**Tree memory.** The working tree stops being a global that belongs to no branch.
Leaving a branch through fufu parks its dirty state — untracked files included —
as an ordinary git stash entry labeled with the branch; arriving brings that
branch's parked entry back. The mechanism is deliberately the one a user would
reach for by hand: every GUI has a stash panel, so a user without fufu sees
exactly what was parked (`fufu: wip on feature-x`) right next to the button that
restores it. It is the stash *dance* that ceases to exist, not the stash — fufu
drives the idiom, and never forgets which entry belongs to which branch, or the
second half.

fufu tracks its parked entries by identity, not position: a ref
(`refs/fufu/parked/<branch>`) records the stash commit's own sha — stable where
`stash@{n}` is not, content-addressed so validation is free, and incidentally
pinning the entry against gc. On arrival fufu finds that exact sha in the stash
reflog and applies only it. It never selects a stash dynamically — by message,
position, or base commit — so a user's own stashes are never touched; the label
is for humans and stash panels, not identification. If the recorded sha is gone
from the stash reflog (popped by hand), the record is invalidated per
cache-not-authority: the reflog is the truth, the ref is the cache.

Invalidation demotes, never deletes. A stash dropped by hand is normally
unreachable and eventually gc'd; here the parked ref simply becomes a timeline
entry — dropped stash, kept by retention, restorable through `ff restore` —
so hand-dropped work stays recoverable by name long after git would have
swept it (jog's `@trash` pattern).

Reconciliation never reconstructs what the user did; it compares states, which
content-addressing makes reliable. At the next invocation: sha gone from the
stash reflog → the user took over (popped or dropped); demote, capture the tree
as it stands. Sha present but its application is a no-op → they applied it by
hand; drop and demote, silently satisfied. Still applicable and clean → offer,
don't inject — the arrival was git's, and completing fufu's arrival ritual
uninvited would cross the regime boundary. Conflicting → the ordinary
held-restore. The one state ambiguity (applied by hand, then edited, entry
kept — indistinguishable from conflicting parked work) is resolved by asking,
not guessing; the capture stream often disambiguates it anyway, since a
snapshot between the apply and the edits shows the intermediate tree.

Arrival never applies blind. fufu first checks the application in memory
(`merge-tree`): clean → applied and dropped; conflicting (the branch moved while
parked) → the entry stays parked and the restore becomes a held rewrite,
announced like any other. A foreign switch gets git's exact semantics — dirty
changes carry over or the switch is refused — and any parked entry simply waits,
visible in `git stash list`. Conflict risk on foreign moves is expected (the two
regimes); the capture floor holds the safety copy regardless.

**Whole-repo undo.** Git has per-ref reflogs but no operation log. Every fufu
operation journals all refs plus the tree state; `ff undo` restores both together.
One timeline for the repository. Because fufu is the primary interface, the journal
is near-complete; raw git mutations are tolerated foreign events that the capture
layer absorbs (reconcile, don't own).

**Futures-aware status.** `git merge-tree` performs merges entirely in memory, so
"would rebasing onto main conflict?" is a free, side-effect-less query fufu runs
continuously. Status reports futures, not just facts: not "12 commits behind main"
but "main moved; your branch rebases cleanly" or "a rebase would conflict in two
files." The user never has to attempt an operation to learn its cost.

### Floor 3 — Rewrite

**Stable change identity.** A rewrite map (old-sha → new-sha, maintained in refs)
lets descendants follow a *change* across amends and rebases, the way jj's change
IDs do. Git's own machinery has been converging on the needed primitives for years —
`merge-tree`, `rebase --update-refs`, `rerere`, autosquash. fufu wires them into an
autopilot.

**Editing at a distance.** jj is *navigational*: to edit a mid-stack commit you
travel to it (`jj edit`), which in git terms means detaching. fufu is *operational*:
you stay at your branch tip and reach back. "Amend this into commit X" applies the
change to X in memory, rebases descendants in memory, and moves refs. You never
leave your branch. Mid-stack editing is a verb, not a place.

**Land-if-clean automation.** Operations attempt themselves speculatively. Clean →
refs move, status says so afterward. Not clean → nothing is touched and the
operation becomes a **held rewrite** (below). Per-branch policy chooses the rung:
report only, offer, or fully automatic. Auto-rebase never chains into anything
outward-facing (never auto-push): textually clean is not semantically clean, and
the status line saying "auto-rebased onto main" is safety information, not
decoration.

## The conflict model: held rewrites

jj stores an unresolved conflict *inside* the result — a commit whose content is a
symbolic merge expression, materialized as markers on demand. That machinery exists
mostly so jj's own always-rebasing engine never has to stop. It cannot cross fufu's
invariant: git trees can't hold an expression.

fufu's observation: for a human, conflicts are operation-shaped, not edit-shaped,
and the user-visible benefit of jj's model is just *scheduling* — the conflict
doesn't interrupt you, and you resolve it when you choose. That benefit survives
translation:

> **A conflict is represented by the operation staying pending, not by a weird
> object in the graph.** jj stores the conflict inside the new commit; fufu stores
> it as the *absence* of the new commit.

When a rewrite (an amend's descendant rebase, a sync onto main) hits a conflict,
nothing is touched. The intent — "this stack has a pending rewrite, conflicting at
commit Y" — is recorded in fufu's state. Both inputs remain ordinary git commits.
You keep working at the existing tip; the pending rewrite replays over whatever you
add. When you choose, `ff resolve` materializes the conflict on your schedule —
and materializes *all of it at once*, not one commit at a time.

Git's sequential stop-fix-continue rebase exists because each replayed commit
changes the base of the next. jj escapes it by letting conflicts live inside
commits and propagate to descendants; fufu runs the same propagation **in
memory**: each step of the held rewrite replays against the previous step's
result, unresolved regions carried forward as literal marker content, conflicts
that later commits resolve anyway vanishing along the way. `ff resolve` then
presents every *surviving* region in the working tree in one editing session —
ordinary conflict markers, each labeled with the commit that owns it
(`<<<<<<< rebasing "add parser options" (3/10)`). Resolutions are absorbed back
into their owning steps (the `ff absorb` machinery pointed at the replay), the
chain re-runs in memory, and the whole rebased stack lands at once: refs move
one time, every landed commit clean, no conflicted state ever existing in the
graph. When same-line conflicts chain across commits the carried markers nest —
jj's notorious ergonomic wart — so `ff resolve --step` keeps the sequential
per-commit mode available, and fufu recommends it when nesting runs deep.

Held rewrites obey cache-not-authority: a held intent is revalidated against the
repository as it is *now* before materializing. If the world moved underneath it —
foreign commits, a rewritten target — fufu recomputes or expires it loudly; it
never replays a stale plan.

What this gives up versus jj: you can't build on the *post-rewrite* state before
resolving, and conflicted commits can't be shipped around — which you'd never want
to push anyway.

**Deferred requires loud.** jj gets away with parkable conflicts only because it
pairs deferral with relentless disclosure. Held rewrites inherit all three of its
disciplines:

1. **Announced at creation** — the moment a rewrite is held, the status channel
   says so, with what conflicts and where.
2. **Pinned until gone** — every status render shows held rewrites until they land
   or are abandoned.
3. **Exits blocked** — `ff push` refuses to publish anything with a held rewrite,
   the way jj refuses to push conflicted commits. (The guard lives on the fufu
   surface: raw `git push` is git — the two regimes — and reconciliation gets
   loud after the fact instead of a hook getting in the way.)

Deferred and quiet is how work rots; deferred and loud is the whole trick.

## Command surface

fufu is the daily driver; git is the escape hatch for genuinely advanced work
(bisect, submodules, plumbing, forensics). "Advanced" means *rare*, not *dangerous* —
the dangerous-but-daily git commands (`rebase -i`, `stash`, `reset`, reflog
spelunking) are exactly what fufu's verbs replace.

**The rule that keeps this honest: every fufu verb must earn its existence by doing
something git's version doesn't** — routing through the op journal, enforcing an
exit guard, engaging tree memory, capturing first. If a proposed verb would behave
identically to its git counterpart, it must not exist; git is right there. The
moment fufu verbs are just spellings, the tool collapses into shell aliases with
extra steps.

The daily surface, roughly a dozen verbs:

| verb | what it does | what it replaces |
|---|---|---|
| `ff status` | state + futures: captured work, held rewrites, "rebases cleanly onto main" | `git status` + attempting things to see if they work |
| `ff commit` | cut a slice from the capture stream; interactive form picks hunks | the `add`/index two-phase ritual (which still works, for those who want it) |
| `ff switch` | branch switch with tree memory | `stash` dances |
| `ff branch` | create/move lines of work | `git branch`/`switch -c` |
| `ff absorb` | fold working changes into the stack commits they belong to; descendants rebase in memory | `commit --fixup` + `rebase -i --autosquash` |
| `ff amend <rev>` | explicit-target editing at a distance | `rebase -i` edit dances |
| `ff sync` | fetch; speculatively rebase onto main; land if clean, hold if not | manual rebase-onto-main ceremony |
| `ff push` | publish, with exits guarded: refuses held rewrites, lease semantics by default | `push --force-with-lease` and prayer |
| `ff undo` | whole-repo undo: refs + tree together | reflog archaeology, `reset --hard` fear |
| `ff log` | the timeline: snapshots interleaved with commits | `reflog` + `log` |
| `ff restore <path> --at <id>` | pull anything back from the timeline | hoping |
| `ff resolve` | all of a held rewrite's conflicts, one editing session, on your schedule | sequential stop-fix-continue rebasing |
| `ff git <args>` | capture-first passthrough; daily forms translate to their fufu verbs | raw git without a net |

### `ff git` and the alias

Like jog, fufu ships a passthrough and recommends the shell alias
(`alias git='ff git'`): typed git snapshots first, then runs. But where `jog git`
is deliberately verb-blind, fufu's passthrough *translates* invocations whose
meaning maps totally onto a fufu verb — `git switch x` engages tree memory,
`git commit -m …` cuts from the capture stream, `git push` meets the exit
guard. Muscle memory gets fufu's guarantees without retraining. The whitelist
is conservative: any flag or form fufu doesn't fully understand falls back to
verbatim passthrough, capture-first, never guessing on someone else's command
line (`git checkout` alone can mean switch-branch or clobber-file; only the
unambiguous forms translate).

The regime boundary is execution path, not spelling: through the alias, git
spellings *are* the fufu surface, while scripts, IDEs, and GUIs resolve real
git on PATH and stay foreign — the same scoping as jog's alias. Hints are
policy, not nag: fufu can mention the native spelling (`tip: that's ff switch`)
once per verb, or never. Translation makes the git spelling a permanent
synonym, not a deprecation.

On a git-free machine, translated forms keep working — they never needed the
binary. Verbatim passthrough is the one thing that still requires git.

Much of fufu's presence is not commands at all: the ambient status channel (shell
integration) speaks at natural pause points — "main moved, rebased you cleanly,
undo if you disagree." The tool is used mostly by *reading* it.

The index: fufu ignores it (commit slices from the stream, with hunk selection at
commit time — jj's actual insight about staging). The index still exists and
`git add -p && git commit` still works, because a boring repo tolerates both. fufu
stops *requiring* the ritual; it doesn't break it.

## Substrate

Rust, on gitoxide (`gix`). The rule that governs execution: **git defines the
semantics; fufu chooses the execution per call-site.**

- **Reads are native from day one.** The ambient channel runs at every prompt,
  and subprocess spawn cost (5–15ms per `git` exec) can't carry it. Refs,
  objects, index and status reads, log walks: in-process. Floor 2's core
  primitive too — merge simulation runs in memory (gitoxide's merge-ort port),
  cached by (base, ours, theirs), so futures recompute only when a ref moves.
- **Writes climb a ladder as trust grows.** Object writes (capture snapshots,
  stash entries) go native early — jog-proven territory. Disk-materializing
  operations (checkout through filters, rebase, fetch/push with auth) start on
  the git binary and go native as coverage earns it.
- **Differential testing is the compatibility contract.** Every native
  operation is checked against git-binary output in CI, permanently. A native
  merge that differs from git's by one edge case silently breaks the invariant;
  compatibility is a standing test suite, not a port milestone.
- **Behavioral compatibility included.** Correct formats aren't enough: if
  `ff commit` writes commit objects directly, the user's pre-commit and
  commit-msg hooks still run — fufu execs them itself. Boring citizenship
  covers behavior, not just bytes.
- **No daemon.** Millisecond cold start plus in-process caching keeps jog's
  no-daemon stance viable: compute lazily at invocation, cache aggressively.

The destination is **completely git-free**: a machine with only `ff` on it is a
fully working development machine, the way a jj user never installs git. The
staging is honest — the daily surface (status, commit, switch, absorb, amend,
sync, undo, log, restore) goes git-free first; the long tail (credential
helpers, filters/LFS, submodules) follows as the substrate matures. The escape
hatch assumes git is present and is expected to be extremely rare; on a
git-free machine, its territory (bisect, plumbing, forensics) arrives inside
fufu over time or waits for a machine that has git.

## Principles, collected

1. **Always a boring git repo.** No state plain git can't represent. Ever.
2. **Reconcile, don't own.** Raw git, GUIs, IDEs, and scripts are first-class
   writers, not edge cases; observe, snapshot around them, absorb them into the
   timeline.
3. **Cache, not authority.** fufu may be abandoned and re-adopted at any moment.
   All fufu state is a rebuildable cache over git; when records and repository
   disagree, the repository wins. Returning is reconciliation, not recovery.
4. **Mechanisms are git idioms, automated.** Park with a stash, record with a
   commit, remember with a ref — whatever a user would do by hand, done
   automatically, so anything fufu leaves behind is legible to every git user
   and GUI. If understanding the repo requires knowing fufu exists, it's the
   wrong mechanism.
5. **Guarantees follow the surface.** Inside fufu, jj's rules: nothing
   interrupts, conflicts are held, everything undoes. Outside it, git's rules,
   exactly — including git's conflicts at git's moments. Witness foreign
   operations loudly; never extend guarantees to them.
6. **Capture before courage.** Automatic rewriting is only acceptable above a layer
   where nothing can be lost and everything is one `undo` away.
7. **Report futures, not just facts.** If an outcome can be known in memory for
   free, status should already know it.
8. **Deferred requires loud.** Announced at creation, pinned until resolved, exits
   blocked. Never deferred and quiet.
9. **Never auto-outward.** Automation may move local refs; publishing is always a
   human verb. Textually clean ≠ semantically clean.
10. **Verbs must earn their existence.** No pure aliases. If git's version is
    identical, the verb doesn't exist.
11. **Self-sufficient surface.** Users and agents never need to learn a fufu↔git
    ref mapping or read internals; if they'd have to, add a flag or verb instead.
12. **Opinionated workflow, neutral repository.** fufu takes positions — rebase
    over merge, malleable unpublished history, routine leased force-pushes. The
    repository stays boring for everyone else; the opinions never leak past the
    push boundary.

## Prior art, and the unclaimed square

- **jj (Jujutsu)** — the workflow North Star; rejected only in its authority model
  (own store, git as backend) and its consequences (detached HEAD, conflicted
  commits as objects, git demoted).
- **git-branchless / Sapling** — proved undo, smartlog, and restack over git
  storage, but drift toward anonymous heads: jj's paradigm again.
- **Graphite** — stays on named branches ("needs restack" is its vocabulary, which
  held rewrites deliberately echo) but solves only stacking, with no capture layer
  underneath.
- **git absorb / hg absorb** — the at-a-distance amend verb, standalone.
- **jog** — fufu's proving ground: the capture floor, shipped and lived-with.

The unclaimed square: continuous capture below, operational (not navigational)
history editing above, and the boring-git-repo invariant enforced throughout. Not
jj-on-git; **git that flies itself.**

## The plan

Phases, not tasks. The ordering is the principles enforcing themselves: capture
before courage (nothing aggressive ships before nothing can be lost), futures
before automation (land-if-clean needs the simulation), exits last (publishing
is the highest-stakes surface). Every phase ends with a tool worth dogfooding
daily — fufu is built by using fufu.

**Phase 0 — Bedrock.** Rust workspace on gix; the native read core (refs,
objects, index, status, log); the differential test harness against the git
binary, which lives forever after; read-only `ff status` and `ff log`. Proves
the substrate and the zero-spawn budget before anything depends on them.

**Phase 1 — Capture.** Floor 1 rebuilt native: snapshot engine, triggers
(shell, agents, editors), per-branch timeline, `ff restore`, retention. jog's
lessons carried over, its code not owed. From here on, nothing can be lost.

**Phase 2 — Time.** The op journal and whole-repo `ff undo`; reconciliation as
a first-class deliverable (cache-not-authority needs machinery, not vibes);
tree memory via driven stash — `ff switch`, `ff branch` — and `ff commit`
cutting slices from the stream. The daily driver exists after this phase.

**Phase 3 — Futures.** In-memory merge simulation, cached; `ff status` starts
reporting futures, not just facts; the ambient shell channel speaks at pause
points. Pure reads — no automation yet, just foreknowledge.

**Phase 4 — Rewrite.** Floor 3: the rewrite map, `ff absorb`, `ff amend` at a
distance, `ff sync` with land-if-clean, held rewrites and `ff resolve`. The
jj-grade workflow lands here, safe because phases 1–3 are underneath it.

**Phase 5 — Exits and adoption.** `ff push` with lease semantics and the held-
rewrite guard; `ff git` passthrough with the translation whitelist and the
recommended alias; install/uninstall; the name and packaging sweep. The tool
becomes recommendable to someone who isn't its author.

**Phase 6 — Git-free.** The long tail moves native — fetch/push, checkout
through filters, hooks exec'd by fufu — until the git binary is an optional
neighbor rather than a dependency. Done when a machine with only `ff` on it is
a working development machine.

## Open questions

- **Snapshot mechanics at fufu's write rate** — jog's capture cadence is
  prompt/command-shaped; land-if-clean automation may want cheaper, more frequent
  capture. In-process object access changes the calculus jog was built under;
  revisit ref layout and caching with gix in hand.
- **Tree memory residuals** — same branch checked out in two worktrees; parked
  entries orphaned by foreign branch deletion; whether to set `status.showStash`
  so plain `git status` mentions parked work.
- **Rewrite-map hygiene** — divergence is settled by cache-not-authority (an
  entry rewritten outside fufu is invalidated, loudly); pruning cadence and the
  map's ref representation are not.
- **Reconciliation triggers** — lazy (rebuild at the next fufu invocation,
  jog-style) versus live (post-commit / reference-transaction hooks). jog's
  no-git-hooks stance is in tension with how fresh the ambient status channel
  can be.
- **Held-rewrite composition** — multiple holds on one stack: ordering, whether
  a new operation queues behind a hold or applies to reality beneath it, and
  expiry/abandon semantics.
- **Resolution absorb-back edges** — edits outside any marked region during
  `ff resolve` need an owner (nearest region's step, or ask); when nested
  markers should trigger the recommendation to fall back to `--step`.
- **Undo across foreign events** — a reconciled foreign mutation becomes a
  journal entry; whether `ff undo` reverts it like any other (probably yes,
  labeled as foreign), and how that's disclosed.
- **Auto-sync policy surface** — per-branch opt-in defaults; whether
  tracked-upstream branches default to "offer" or "auto."
- **Name/packaging sweep** — `fufu` and binary `ff` against crates.io, npm,
  Homebrew, apt before anything ships. (Known neighbors: `ffuf` the web fuzzer,
  `fuf` an obscure file browser — distinct, but check registries.)
- **Substrate maturity tracking** — which daily verbs must stay binary-backed at
  v1; gitoxide's push support and filter pipeline are the pacing items for
  git-free. (Language/runtime itself is settled: Rust on gitoxide — see
  Substrate.)
