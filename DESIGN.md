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

Automation is not foreign by nature, only by habit. A script, a CI job, or an agent that calls `ff` is inside the surface with everyone else, and gets everything the surface promises. Today they reach for git because fufu gives them nothing better to reach for; the machine surface (below) is the work of making that choice obvious.

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

**The open change.** jj makes the working copy a literal commit, eagerly created
empty and continuously amended. fufu keeps the guarantee and drops the object:
the working tree *is* the open change, its history is the capture chain, and no
commit exists until the change closes. `ff commit` is the close — build the
tree (`add -A` semantics), write the commit, the branch advances, status is
clean. A clean tree makes the close a no-op: **no empty commit is ever
created** — jj's placeholder commits are exactly the kind of state a boring
repository shouldn't contain. Descriptions are two-phase: `ff commit -m`
describes the change being closed, `ff start -m` the change being opened — a
pending description parked per branch until its close; bare `ff describe`
edits it, `ff describe <rev>` rewords what's already closed. An undescribed
close is legal ("(no description)", jj-style); hygiene enforces at the exit,
where `ff push` flags undescribed commits rather than letting them past the
boundary.

`ff start` (alias `ff new`) always begins a new line of work — a fresh
branch, every time. The verbs carve cleanly: `ff commit` records, `ff
switch` resumes, `ff start` begins. A tree belongs to its branch and every
arrival materializes the destination's own, so starting is always travel:
the open change parks where it was and the new branch opens clean, exactly
as a switch would leave things. Bare, it forks trunk; a `<rev>` forks there
instead. `-m` describes the change being opened, `-b` names the minted
branch. Work already begun moves onto its own branch through `ff commit -b
<name>`, which closes it onto a fresh branch and leaves the current one
standing — no verb carries a working copy across a fork. `ff edit` sits
adjacent: it targets *commits* (a session), `ff switch` targets *branches*,
and `ff edit <branch>` simply behaves as `ff switch`.

**Trunk is known.** Several verbs need to know what "main" is: `ff sync`
rebases onto it, futures in `ff status` measure against it, and bare `ff
start` starts from it. fufu resolves trunk once per repo: config first
(`fufu.trunk`, set through `ff config trunk <branch>`), heuristics when
unset — `origin/HEAD` if the remote declares it, else a lone local `main`
or `master`, else a lone local branch of any name. Ambiguity is an
error naming the candidates, never a guess. Trunk may live only on the
remote: with no local `main`, bare `ff start` forks straight from the
fetched tip — no local trunk branch is required or created.

**Someone else's branch.** Reviewing work that isn't yours is arriving at a
branch that already exists — `ff switch`'s job, not `ff start`'s, which
would fork a branch of your own beginning at their tip. The
`<branch>@<remote>` spelling is both address and freshness request:
`ff switch feature-x@origin` fetches, then lands you there. Switch may
create a branch, never move one: a name that resolves only on the remote is
fetched and created here, while a branch already local stays offline and
untouched. `@<remote>` is the opt-in that permits moving it — fast-forwarding
where the remote fully contains it, stopping to name the commits where it
doesn't, rather than discarding work no snapshot took.

**Branches without ceremony.** Every head is a real ref under `refs/heads/`
from the moment it exists — HEAD never detaches, and branches auto-move as
commits land, because that is git's own behavior once HEAD is attached
(contrast jj's bookmarks, which sit still until told). Work doesn't wait for a
name: every `ff start` mints an **anonymous branch** — a
real branch with a generated name under a reserved prefix (`ff/quiet-lake`) —
unless `-b` names it at birth; `ff branch <name>` claims it later: a rename that carries the capture
chain, the parked entry, and fufu's metadata along, which is the part a bare
`git branch -m` would orphan. A `-b <name>` flag rides the change verbs
on the same axis as `-m`: on `ff describe` it always *renames* — the claim,
made inline, and the one form that renames proper names too. On `ff start`
it names the branch being minted — every `start` creates one, so there is
nothing to decide. On `ff commit` it names the branch the closing change lands on,
and the reserved prefix makes the meaning decidable rather than guessed: a
placeholder — fufu-named — is claimed in place, while a branch the user
deliberately named is never renamed implicitly, so a fresh branch is created
instead. Thus `ff commit -b` on a named branch lands the closing change on the
new branch and leaves the current one where it stands (the "this shouldn't go
on main" rescue), and on a placeholder simply names the line you were
already on. Creating a branch never moves the branch it forks from: the old
branch keeps its tip — advanced only by its own change's close, never by the
new branch's commits. A `-b` name that already exists is an error, never a
reuse. To every foreign tool an anonymous branch is
ordinary; no push refspec matches it by accident, and naming one is the
natural "this is real now" gesture at the publish boundary.

Target resolution is uniform: every `ff start` target forks — continuing an
existing branch is `ff switch`'s job, never `start`'s, so there is nothing
to guess. Git permits no divergence inside one ref and no
`foo/2` beside `foo` (a ref is a file, and can't also be a directory), so
every fork is its own ref; "forked from main" is metadata recorded at fork
time and shown at display time, never encoded in the name.

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

**Mid-stack editing, two reaches.** jj edits a mid-stack commit by traveling to
it (`jj edit`), which in git terms means detaching HEAD. fufu keeps the reach and
drops the detachment. The short reach is at a distance: `ff absorb` (aimed with
`--into <rev>`) applies working changes to a commit in memory, rebases its
descendants in memory, and moves refs — you never leave your tip. The long reach
is a session: `ff edit <rev>` parks the tip's state through tree memory and
materializes `<rev>`'s tree into the working tree, HEAD staying attached to the
branch the whole time. You edit the commit's actual content — the thing distance
can't give you — and `ff done` amends it, restacks descendants in memory, and
returns you to tip with the parked state restored. A conflicting restack holds
like any other rewrite. Travel happens in tree-space, never in ref-space: to
plain git a session is nothing more exotic than a dirty working tree on the
branch, abandoning fufu mid-session leaves exactly that, and the capture floor
holds the tip state regardless.

A session must end — `ff done` is the only place the amend-and-restack fires —
but ending it is not always the user's chore. Any verb that needs the branch
to move on treats an open session as land-if-clean automation: `ff start` or
`ff commit`, mid-session or arriving back at a branch whose session was
parked, attempts `ff done` speculatively first. Clean → the edit lands,
descendants restack, the verb proceeds, and status says what happened.
Conflicting → the verb stops *in* the session: the session state is
materialized (back onto the edited commit's content, if you'd switched away)
and fufu says exactly what's going on — editing which commit, conflicting
where — with the exits listed: finish and `ff done`, `ff resolve`, abandon,
or switch away to defer again. This is not automation guessing intent:
`ff edit` declared the operation, `done` is its bookend, and finishing a
declared operation when finishing is free is the floor's theme again — just
work when it can just work, stop for the user when it can't. `ff switch`
remains the deliberate "later": leaving a branch parks the session, note
included, like any dirty state, and status pins it until return. An explicit
abandon (`ff done --abandon`) drops the session and restores the parked tip;
the session's edits stay in the capture chain regardless.

**Land-if-clean automation.** The theme of the whole floor: **just work when it
can just work; stop for the user only when it can't.** Operations attempt
themselves speculatively. Clean → refs move, status says so afterward. Not
clean → nothing is touched and the operation becomes a **held rewrite** (below). Per-branch policy chooses the rung:
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
(`<<<<<<< rebasing "add parser options" (3/10)`). `ff done` ends the session:
resolutions are absorbed back into their owning steps (the `ff absorb` machinery
pointed at the replay), the chain re-runs in memory, and the whole rebased stack
lands at once: refs move
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

The daily surface. Where the workflow is jj's, the vocabulary is too: `edit`
and `describe` are deliberate imports, and jj's `new` survives as the alias for
`ff start`, with `switch` staying underneath them as the general movement verb.

| verb | what it does | what it replaces |
|---|---|---|
| `ff [-m <msg>]` | take a manual snapshot — bare `ff` is the snapshot verb, jj-style | `wip` commits, stash-as-backup rituals |
| `ff status` | `ff log` cropped to two rows — the open change and the commit under it — with the diffstat between them, plus futures: held rewrites, "rebases cleanly onto main" | `git status` + attempting things to see if they work |
| `ff commit` | close the open change: commit the working tree (`-m` describes what's closing, `-b` names where it lands — claims a placeholder, else a new branch); interactive form picks hunks — a slice cut from the stream | the `add`/index two-phase ritual (which still works, for those who want it) |
| `ff describe [<rev>] [-m <msg>] [-b <name>]` | reword any commit's message (`-m` inline, else the editor) — bare form edits the open change's pending description; `-b` renames the branch (the claim, inline); descendants restack in memory | `commit --amend` at the tip, `rebase -i` reword dances anywhere deeper |
| `ff start [<rev>] [-m <msg>] [-b <name>]` (alias `ff new`) | begin new work on a fresh branch, always: bare forks trunk, a `<rev>` forks there; the open change parks and the new branch opens clean; `-m` describes the change being *opened*, `-b` names the minted branch (else anonymous); never an empty commit | `git switch -c` + the stash dance |
| `ff switch <branch>[@<remote>]` | branch switch with tree memory; `@<remote>` fetches first and lands you on a synced copy | `stash` dances, `fetch` + `switch -c --track` |
| `ff branch` | move/rename/delete lines of work — journaled, undoable, parked-entry-aware; `ff branch <name>` claims an anonymous branch, capture chain and parked state carried along | `git branch` bookkeeping |
| `ff absorb` | fold working changes into the stack commits they belong to (`--into <rev>` aims a specific one); descendants rebase in memory | `commit --fixup` + `rebase -i --autosquash` |
| `ff edit <rev>` | editing session on any commit: parks tip state, materializes `<rev>`'s tree, HEAD never moves; given a branch name it simply is `ff switch` | detached-HEAD `rebase -i` edit dances |
| `ff done` | finish the current session (`edit` or `resolve`): absorb the edits, restack in memory, land, return to tip | `rebase --continue` ceremony |
| `ff sync` | fetch; speculatively rebase onto main; land if clean, hold if not | manual rebase-onto-main ceremony |
| `ff push` | publish, with exits guarded: refuses held rewrites, lease semantics by default | `push --force-with-lease` and prayer |
| `ff undo` | whole-repo undo: refs + tree together | reflog archaeology, `reset --hard` fear |
| `ff log` | changes as the spine, jj-style: the open change (`@`) atop the commit walk (`●`), each commit wearing its newest snapshot's id | `reflog` + `log` |
| `ff evolog` | the open change's snapshot chain, newest first — the drill-in behind `ff log`'s letters column | `reflog` spelunking |
| `ff session [start\|end] [<label>]` | open or close a named span of the capture chain; snapshots taken inside it carry its id, so a stretch of work is one thing you can ask about | nothing — git has no concept of work that isn't a commit |
| `ff restore <path> --at <id>` | pull anything back from the timeline | hoping |
| `ff trim` | drop snapshots past the keep window — trash-first, so the last trim is itself undoable; rides an ff command daily, so retention enforces itself | remembering to prune, or quietly never pruning |
| `ff resolve` | all of a held rewrite's conflicts, one editing session, on your schedule | sequential stop-fix-continue rebasing |
| `ff git <args>` | capture-first passthrough; daily forms translate to their fufu verbs | raw git without a net |
| `ff config` | every setting in one place: typed registry, defaults on display, values validated before they land | `git config` guesswork and doc-spelunking |
| `ff update` | move this binary to the latest release: verified download, atomic swap; a passive lane checks ~daily and auto-installs, or prints a one-line notice | re-running installers, stale binaries |
| `ff doctor` | verify the net: chains, identity, reflogs, gc guard, objects, wiring, update — `--fix` repairs exactly the gc keys | "is this thing even on?" doubt |
| `ff explain <id>` | what an error id means, and the ways out of it | searching the message text |
| `ff watch` | stream journal entries as they land, newline-delimited JSON | polling `git status` in a loop |
| `ff completions <shell>` | shell completion, with branches, revs, and snapshot ids resolved live | hand-rolled dotfile fragments |

### Presentation conventions

Snapshot ids are spelled in jj's reverse-hex alphabet: hex digit value `i` maps to `"zyxwvutsrqponmlk"[i]`, so `0` → `z` down through `f` → `k`. The letter range k–z shares no character with hex, so a snapshot id can never be misread as a commit sha, and parsers can accept both without ambiguity. Everywhere a snapshot id is input (`ff restore --at`), the letters spelling is accepted alongside raw hex. Ids never contend with times for a position, because the grammar gives a time exactly one spelling — see One target grammar.

Snapshot id columns highlight the shortest unique prefix: bold what you can type, dim the rest. The uniqueness domain is exactly the set `ff restore --at` resolves against — the current branch's live and trash chains — so the bold prefix is precisely what restore accepts unambiguously. That domain is materialized per chain under `<common-dir>/fufu/ids/`, appended by capture and rebuilt whenever the chain tip moves out from under it, so highlighting and restore read one list rather than two code paths agreeing. Commit shas get no highlighting: they display as a plain 7 characters (7 is effectively always odb-unique at this repo's scale, and git resolves any rare ambiguity when one is pasted); the snapshot column is where the highlighting pays. One palette serves the whole tool, and it colors **roles, not commands**: snapshot ids magenta, commit shas blue, ages cyan, the working-copy `@` green, insertions green and deletions red, rails and asides dim, plus three verdict colors — green for clear, orange for trouble, blue for ahead-of-upstream. Prose stays plain; color marks the tokens you scan for (hex, ids, times) and the verdicts you act on. That is why `ff doctor`'s WARN, a rebase that would conflict, and a check that failed are all the same orange: one color per meaning, wherever the meaning turns up. Color is redundant encoding and never the only encoding: every verdict carries a word or a glyph saying the same thing, so `NO_COLOR`, a monochrome terminal, and a screen reader cost the reader decoration and never information.

`ff status` is that same picture cropped to two rows: `@`, the open change, and the commit under it, with the diff between them hanging on the rail that joins them — one line per file: a change-kind letter, the path, insertions and deletions counted separately, and a width-scaled `+`/`-` bar. The two counts stay apart rather than summing to git's single total because "18 changed" is the one number nobody acts on, and the letter earns its column because no bar can tell a new 40-line file from one that grew by 40, while binary and mode changes have no numbers to draw at all. There are no sections: a file is ignored or it is listed. Conflicts keep a block of their own — that is a state, not a staging distinction — and the futures line closes the block. The futures line spends the three verdict colors exactly as the shared rule defines them — dim when there is nothing to say, blue when the branch is only ahead, green when upstream moved and the rebase is clean, orange when it conflicts — and the verdict outranks ahead-ness when both are true, because it is the half that needs a decision. The palette is 256-color and deliberately desaturated: the base sixteen have no orange at all, and saturated glyphs make the dim rails beside them look broken — anstream downgrades on terminals that can't do 256. Which palette is `fufu.theme`: `muted` by default, `vivid` for the saturated cut, and `terminal` to drop to the base sixteen and inherit whatever the user's own terminal theme defines — the one honest choice for someone who has already tuned their colors and wants every tool to respect them. `--json` carries the model rather than the crop (see The machine surface): the same `changes` array, plus `open`, `parent`, and the futures the two-row view compresses into one line.

The log family (`ff log`, `ff evolog`, `ff log --ops`) pages on a TTY, git-style: `fufu.pager` config, then `FF_PAGER`, then `PAGER`, then `less`, whitespace-split with no shell quoting. `LESS=FRX` and `LESSCHARSET=utf-8` are provided when unset (quit if one screen, keep ANSI colors, don't clear the screen). Piped output and `--json` never page; a pager that fails to spawn falls back to direct printing, silently. Color follows anstream's auto-detection — `NO_COLOR`, `TERM=dumb`, and non-TTY stdout all disable it, and the decision is made against the real terminal before the pager pipe wraps it. No `--color` flag yet; the knobs that exist are the ambient ones.

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

Staged and tracked are git states fufu has no concept of. The working tree *is*
the change, whole: `.gitignore` is the only line left — ignored is invisible,
everything else is already in the commit about to be cut. Selection survives
where it belongs, at commit time (`ff commit` picks hunks — jj's actual insight
about staging): a choice made once, not a state maintained. That makes the
ignore file load-bearing in a way git never asked it to be — git lets an
unignored `target/` sit untracked and harmless, fufu commits it — so an
arriving file that is large or unusual is the one thing worth a word before it
lands.

Git's own commands still write the index, and `git add -p && git commit` still
cuts exactly what it always did, because a boring repo tolerates both. fufu just
never reads it: partial staging is invisible to `ff status` and subsumed by
`ff commit`, which takes the tree.

### `ff trim`

Retention with an undo. `ff trim` drops the oldest suffix of every chain past `fufu.keep` (90 days by default), and the op journal rides the same cutoff. Each chain's pre-trim tip is written to `refs/fufu/trash/<branch>` before a single ref moves, so the last trim is itself recoverable; survivors keep their trees, messages, and dates byte-for-byte, only parent slots relink, and the reflog is replayed with the original times so `@{n}` and `@{time}` stay truthful. A crash mid-trim leaves a shorter-but-valid chain and the full pre-trim state in trash.

The earned existence is the automatic half: a safety net whose upkeep is a chore is a net that quietly rots. A trim rides an ff command at most once per `fufu.autoTrim` (daily by default), per repository, and it runs **inline** — the engine is native, so there is no child to spawn and nothing to wait on. The hot path is one read of a per-repo stamp beside the common git dir; config is consulted only when the stamp says a trim might be due, and the stamp is written *before* the trim runs, so a failure retries on the cadence rather than on every command. The one thing the automatic lane deliberately skips is manual trim's `git gc --auto` nudge: that would put a spawn on the commands that carry the lane, and bare `ff`, `ff git`, and `ff hook` stay provably spawn-free. A hand-run `ff trim` nudges whether or not it dropped anything: native writes never trigger auto-gc, so without that nudge nothing ever packs the store, and an unpacked store taxes every chain walk long before retention is due. `gc --auto` is self-limiting, so the nudge costs nothing until git itself thinks packing is worthwhile. `fufu.autoTrim false` leaves trimming entirely by hand.

### `ff config`

jog's config command, carried over: no subcommands, arity decides. Bare `ff config` lists every setting with its value, its meaning, and a `(default)` marker; a key gets; key plus value sets; `--unset` returns to the default; `--global` widens a set or unset to every repo. Storage is plain git config under `fufu.<key>`, so `git config fufu.keep` and fufu never disagree, and precedence is git's own — local over global, environment over both.

The earned existence: git config can't say what settings fufu has, what they default to, or whether a value will parse. Every fufu reader falls back to its default on a value it can't read, so a typo'd `fufu.keep` looks set and does nothing. `ff config` closes that gap — a typed registry (size, duration, command, cadence, bool), validation through the same parsers the readers use before anything touches disk, and exit codes that mean something: 0 done, 2 usage or bad value, 1 real failure. Writes are native like everything else: gix's lossless config file, git's own `config.lock` convention, atomic rename, comments preserved. Zero spawns, including the write path.

### `ff update`

jog's self-updater, carried over. The earned existence: fufu ships six release targets, a tap, and two install scripts — plenty of ways to install, and until now nothing that keeps an installed binary fresh. `ff update` moves the running binary to the latest GitHub release: pick the platform asset, stream it through sha256 against the release's `checksums.txt`, extract, and atomically rename over the executable (unix rename never touches the busy inode, so there's no ETXTBSY; windows does the `.old` two-step and rolls back on failure). Not every install is fufu's to touch: Homebrew binaries get pointed at `brew upgrade fufu`, source builds at `cargo install` — and the official/source distinction is a compile-time marker the release workflow sets, because on linux `current_exe()` is already symlink-resolved and path inspection cannot tell a dogfood build from an official one.

The passive lane keeps installs fresh without being asked. Official binaries (never dev, dogfood, or test builds; never under CI) spawn a detached `ff update --check` at most once per `fufu.updateCheck` (default daily) — the one sanctioned self-spawn in a zero-spawn binary; it refreshes a small cache file under the user cache dir and exits. Foreground commands read that cache: with `fufu.autoUpdate` on (the default) a newer release installs itself silently in the background — the in-flight command finishes on the old inode, the next one runs the new binary; with it off, a one-line notice lands on stderr instead. Three throttles keep it polite: the cadence gates the checks, auto-install probes retry at most daily, and a release is announced at most once, ever. `fufu.updateCheck false` turns the whole machinery off. The trust root is deliberately plain: HTTPS to GitHub plus the release's sha256 — the same root the install scripts already rely on.

### `ff doctor`

jog's doctor, carried over. The earned existence: a safety net you can't inspect isn't trustworthy — every floor can degrade silently (a chain moved by something that isn't fufu, a reflog that never got created, the gc guard deleted from local config, hooks never installed, a stale binary), and without a doctor the first notice is the day the restore you needed isn't there. One command reads the whole net: the engine (chains and their ages, the snapshot identity on every tip, reflogs, the gc guard, journal health and pending foreign drift, settings validated through the same parsers the readers use, the object store's loose-versus-packed split, a trim preview and the auto-trim clock), the wiring (claude hooks, the shell alias, and a triggers check that warns when nothing at all feeds the capture floor — a silent engine feels safe while capturing nothing), and the update cache.

Three row levels, jog's shape: `ok` counts nothing, `info` is news not a problem, `WARN` is a finding. Findings drive the exit code — 0 healthy, 1 findings — so scripts and CI can gate on it, and `--json` emits the same rows for machines. Read-only by design: doctor reports the drift the journal will absorb and never absorbs it, takes no snapshot, reconciles nothing. The one consented write is `--fix`, which repairs exactly the two gc reflog-expiry keys — rewriting wrong values where the lazy guard only ever appends missing ones — and nothing else.

## The machine surface

The regimes sort by execution path, and that sorting has been quietly exiling automation: a script, a CI job, an editor plugin, or an agent reaches for git because fufu offers it nothing better to reach for, and takes git's guarantees instead of fufu's. Nothing about a script deserves less than a person at a prompt. So the machine is a first-class reader, and the surface it reads has to be good enough that automation chooses it.

Agents make this urgent rather than merely tidy. An agent edits at machine rate, commits nothing for an hour, and cannot read a status block built to be scanned by eye. Capture already runs before every action it takes — fufu holds a record of that work no other tool has. What is missing is a way to ask for it.

**One model, every surface.** A verb computes one data model, and the human rendering and the JSON rendering are both consumers of it, never translations of each other. `--json` is therefore not a mirror of the human layout: `ff status` crops to two rows because that is what an eye wants, while its JSON carries the model whole. That is what keeps the two from drifting a release apart, and what makes any further surface — an MCP server, a completion source — a thin shell over one contract rather than a second implementation with its own opinions. The JSON is enveloped and versioned (`{"ff": 1, "cmd": "status", …}`) so a script can assert what it is talking to. Notices belong to the model too, not to the margin beside it: anything fufu would tell a person, a script reads as data.

**Every stop names its exits.** Every error carries a stable id, one line of what happened, and the ways out. Prose gets reworded; ids do not, so a script branches on the id instead of matching a sentence, and `ff explain <id>` turns one back into prose on demand. The exits are the accessibility half — fufu is a workflow shift, and where a newcomer bounces is the first stop whose way out they cannot see.

Exit codes carry the same verdict at shell resolution: **0** done, or yes; **1** no — it failed, or the check's answer is negative; **2** the command line was wrong; **3** held — nothing was touched and a human decision is required. `3` is the code git has no use for, because only a tool with land-if-clean produces that outcome: `ff sync` exiting 3 is a scriptable "main moved and it needs you."

**Every ask has a non-interactive answer.** Wherever this document says fufu asks — the ambiguous parked entry, an undescribed close, an unusual file about to land — a flag supplies the answer up front, and in a non-interactive environment (`FF_NONINTERACTIVE`, or stdin that is not a terminal) the question becomes a structured error naming that flag. No verb ever blocks on a prompt or an editor with nobody there to answer it. `FF_READONLY` completes the pair from the other side: a mode refusing every mutation, so a supervisor or a CI job can be certain a read stayed a read.

**Sessions.** A session is a named span of the capture chain: snapshots taken while one is open carry its id, and the span is exactly the snapshots carrying it. Anything that snapshots can open one — `ff session start <label>` before a long afternoon, `FF_SESSION` in a script's environment, the agent trigger opening one per agent session without being asked. Nothing has to close cleanly: an unended session stops where its labeled snapshots stop, so a crash costs nothing. The id rides a trailer on the snapshot commit — git objects, legible to anyone who reads them — with a per-repo index as the usual rebuildable cache.

Grouping is the whole feature, and it buys a question nothing answers today: what did that entire stretch of work change? Agents sharpen it — an agent edits for twenty minutes and commits nothing, so its work exists only as capture — but it is not an agent question, and a person's afternoon groups the same way. Sessions are named by flag, never by address (`ff log --session`, `ff session diff <label>`): they name a set, and the target grammar below names points. Keeping those apart is what leaves room for a set language to arrive later with sessions as a natural member, instead of retrofitting one into a grammar that never meant to hold it.

**One target grammar.** Every argument naming a point in history goes through one resolver, and it takes every namespace fufu knows: commit shas, snapshot ids in letters or hex, branch names including anonymous ones, `<branch>@<remote>`, `@` for the open change and `@-` for the commit under it, `trunk`, and git's own suffixes and `@{…}` times. It is deliberately `gitrevisions(7)` plus fufu's namespaces rather than a replacement — respelling ancestry would make the grammar a dialect instead of a superset, and fail rule ten. Ranges are not in it: no verb takes one yet, and staying out of range and set syntax now leaves that vocabulary unspent for whatever set language sessions eventually want. The earned existence is uniformity, which git does not have — `git log <rev>`, `git checkout <rev>`, and a pathspec resolve by three different rules — and it is principle 11 mechanized: a script constructs a target without a mapping table because there is one grammar to know.

The grammar is unambiguous by construction, not by precedence: **one spelling per meaning**, so no token can belong to two namespaces and no resolution order has to break a tie. A time is `@{…}` and only that — bare compact ages (`3d`) and date words (`noon`) are not times here, which is what lets `123d` be the hex id it looks like and retires an entire class of shadowing rather than documenting it. Ergonomics return through typed positions: a flag whose kind *is* a time (`--since`, `--before`) takes bare `3d` freely, because nothing else can appear there. When a token has no available reading, the error names the other spelling — `3d` is an id here; a time is `@{3d}` — which is principle 16 with nothing left to guess between.

The same discipline governs arguments generally: **a positional argument has exactly one kind.** Where a verb needs a second kind it takes a flag, which is why `ff restore <path> --at <id>` puts the path in the position and the point behind a flag. fufu never inherits git's `--` disease, where one position means a rev or a path and a separator arbitrates.

Verbs still mean a *kind*, and a kind mismatch redirects rather than refuses. `ff switch <sha>` has exactly one sensible reading, so fufu mints an anonymous branch there and says so, naming `ff branch <name>` to claim it and `ff start` as the verb that meant it; `ff edit <branch>` already redirects the other way. Acting is not guessing: one available reading is taken and announced, while more than one is an error naming the candidates — which is why trunk resolution still refuses to pick.

**Extension.** Three mechanisms, all of them git idioms:

- `ff <name>` runs `ff-<name>` from PATH when no built-in verb matches — git's own extension model, with the repository, the machine contract, and the open session handed down in the environment.
- `ff watch` streams journal entries as they land, newline-delimited JSON. The op journal is already an event log; this gives it subscribers — status lines, editor plugins, dashboards, agent supervisors. It is not a daemon: it is a foreground process the user started, and it holds no authority.
- `ff-core` is a library before it is a binary. Publishing it is the deepest hook and the largest promise, so it waits until what it would freeze has stopped moving.

The rule that keeps extension from eating the invariant: **extensions read fufu state and call fufu verbs; only fufu writes fufu state.** A plugin editing `refs/fufu/*` is a second author of a cache whose entire safety argument is that it has one — principle 3, with the author named.

Hooks are the oldest mechanism and the widest: everything feeding the capture floor is `ff hook`, and the agent trigger's contract *is* its extension point — always exit 0, never veto, fail silently, `FF_DEBUG=1` to see why. That contract is what makes the trigger safe to install into a tool fufu has never heard of, so it is published rather than reimplemented per vendor: known agents get a thin installer, anything else calls the generic trigger with its payload on stdin. A hook never vetoes, ever; the wish to stop an agent from touching a branch is policy, and policy lives in config.

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
staging is honest — the daily surface (status, commit, describe, new, switch,
edit, absorb, sync, undo, log, restore) goes git-free first; the long tail (credential
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
    The reader may be a script or an agent, and is owed the same self-sufficiency.
12. **Opinionated workflow, neutral repository.** fufu takes positions — rebase
    over merge, malleable unpublished history, routine leased force-pushes. The
    repository stays boring for everyone else; the opinions never leak past the
    push boundary.
13. **No linear-time costs.** What a verb costs must not depend on how much has
    accumulated — how deep the history, how long the snapshot chain, how many
    operations the journal holds. Capture runs before every agent action, so a
    cost that grows with what capture has already recorded compounds against
    itself. Growth is measured as cost per 10× of an axis and gated in CI
    (`scripts/bench/`); a verb genuinely allowed to scale — scanning N files
    costs O(N) for everyone — declares that in the table rather than escaping
    the check. Absolute speed is the machine's business; flatness is fufu's.
14. **One model, every surface.** A verb computes one data model; every rendering of it — human, JSON, whatever comes later — consumes that model rather than translating another rendering. Whatever a person can read, a script reads as data.
15. **Every stop names its exits.** Errors carry a stable id, what happened, and the ways out. Machines branch on the id and people take the exit; neither has to read prose that changed last release.
16. **Do what they meant, and say so.** Where there is one sensible reading, act on it and announce it — a kind mismatch redirects, a clean rewrite lands. Where there is more than one, stop and name the candidates. Guessing between meanings is what's forbidden, not acting on the only meaning there is.

## Prior art, and the unclaimed square

- **jj (Jujutsu)** — the workflow North Star; rejected only in its authority model
  (own store, git as backend) and its consequences (detached HEAD, conflicted
  commits as objects, git demoted).
- **git-branchless / Sapling** — proved undo, smartlog, and restack over git
  storage, but drift toward anonymous heads: jj's paradigm again. (fufu's
  anonymous *branches* are the opposite corner — heads that are real refs from
  birth, merely not yet named.)
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

The machine surface is not a phase but a constraint on every one: one model with two renderers is a Phase 0 substrate decision, error ids and exit codes arrive with the verbs that raise them, sessions ride the capture floor in Phase 1, `ff watch` follows the journal in Phase 2, and the generated surfaces — MCP, completions — land with adoption in Phase 5.

**Phase 0 — Bedrock.** Rust workspace on gix; the native read core (refs,
objects, index, status, log); the differential test harness against the git
binary, which lives forever after; read-only `ff status` and `ff log`. Proves
the substrate and the zero-spawn budget before anything depends on them.

**Phase 1 — Capture.** Floor 1 rebuilt native: the snapshot engine, with bare
`ff` as the snapshot verb (jj-style — `ff [-m <msg>]` is a manual snapshot,
and every other ff command captures first); the per-branch timeline
interleaved into `ff log`; `ff restore`; manual retention (`ff trim`). The
`ff git` passthrough with its translation whitelist and the recommended
alias move up from Phase 5: the translation layer grows with the verbs, and
anything reaching for git grabs fufu instead from day one. Triggers are the
capture-first commands, the alias, and agent hooks (Claude Code); editor
integration is deferred until a real need shows up. jog's lessons carried
over, its code not owed. From here on, nothing can be lost.

**Phase 2 — Time.** The op journal and whole-repo `ff undo`; reconciliation as
a first-class deliverable (cache-not-authority needs machinery, not vibes);
tree memory via driven stash — `ff switch`, `ff start`, `ff branch`, anonymous
branches and the claim-rename — and `ff commit` closing the open change (fufu's
first index write: a close must leave `.git/index` matching the new HEAD, or
foreign `git status` shows phantom changes). The daily driver exists after this phase.

*Phase 2 implementation notes (shipped Aug 2026).* The journal is a commit
chain at `refs/fufu/journal`: parent 1 the previous entry, parents 2..n the
commits the entry references — reachability is the gc pin, and the CAS append
is the op serialization lock. Every mutating verb journals write-ahead
(planned post-state before mutating); a crash between append and mutation is
labeled "may not have completed" by the next reconcile. Foreign motion is
absorbed as one `foreign` entry per pass, quoted with git's own reflog
messages, undoable and labeled; the notice stays pinned in `ff status` while
the journal tip is foreign. Parks are byte-shaped `git stash push -u -m
"fufu: wip on <branch>"` entries (differentially proven), tracked by identity
in `refs/fufu/parked/<branch>`; drop is reflog surgery matching `git reflog
delete --rewrite` byte-for-byte. Indexes fufu writes carry a synthesized TREE
cache extension (gix can't attach one, but its serializer is public — the
spike succeeded, so the staged-known-clean shortcut survives fufu's own
writes); stat data carries over where (path, id, mode) survived. Documented
divergences from git: fufu-written indexes are V2/V3 with no
UNTR/FSMN/REUC carry; branch renames replay the reflog but write no
"Branch: renamed" line (its old and new values are equal, which the ref
transaction machinery drops) and the first replayed line's previous-oid
column is null; parks refuse intent-to-add entries exactly as `git stash`
does; `ff commit` during a foreign merge/rebase refuses, pointing at git,
until Phase 4 owns merges. Journal retention rides the same `fufu.keep`
knob through `ff trim` — trash-first at `refs/fufu/trash/@journal`, pin
parents preserved verbatim, prev links rewritten — and the oldest survivor
becomes the undo floor. Petnames are `ff/<adjective>-<noun>` from embedded
wordlists. Two-phase descriptions live in plain JSON files under
`<common-dir>/fufu/branch/<branch>` (pending description + fork base),
journaled on every change so `ff undo` restores the text.

**Phase 3 — Futures.** In-memory merge simulation, cached; `ff status` starts
reporting futures, not just facts; the ambient shell channel speaks at pause
points. Pure reads — no automation yet, just foreknowledge.

**Phase 4 — Rewrite.** Floor 3: the rewrite map, `ff absorb`, `ff describe`,
`ff edit` sessions with `ff done`, `ff sync` with land-if-clean, held rewrites
and `ff resolve`. The jj-grade workflow lands here, safe because phases 1–3 are
underneath it.

**Phase 5 — Exits and adoption.** `ff push` with lease semantics and the held-
rewrite guard; the name and packaging sweep. (The `ff git` passthrough and
alias shipped with Phase 1; by here the translation whitelist has grown with
every verb.) The tool becomes recommendable to someone who isn't its author.

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
- **Anonymous-branch hygiene** — unnamed branches accumulate; `ff branch`
  segregates them in listings (Phase 2) and the name scheme and metadata
  home are settled (`ff/<adjective>-<noun>`; JSON under
  `<common-dir>/fufu/branch/`), but a tidy of merged or abandoned anonymous
  branches remains open.
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
- **Edit-session boundaries** — fufu's own verbs are settled (close verbs
  attempt `done` land-if-clean; `ff switch` parks the session; explicit
  abandon), but: what a *foreign* switch or commit does to an open session;
  how capture attributes mid-session snapshots (to the target commit, not the
  tip); whether editing a published commit warns or refuses; and how a
  session-turned-held-rewrite composes with the parked tip state.
- **Undo across foreign events** — settled in Phase 2: a reconciled foreign
  mutation is a journal entry, `ff undo` rolls it back like any other, the
  report labels it as a change made outside fufu. One rough edge stays open:
  a foreign entry records no pre-state index (fufu wasn't running), so its
  undo restores a clean index at the pre-state HEAD.
- **Auto-sync policy surface** — per-branch opt-in defaults; whether
  tracked-upstream branches default to "offer" or "auto."
- **Agent adoption** — the machine surface makes fufu usable by an agent; what
  makes an agent reach for `ff` instead of `git` is a separate question.
  Candidates: guidance delivered into the agent's own instructions at hook
  install, an MCP server that is simply the path of least resistance, and
  translating agent-issued git the way the alias translates a person's.
- **Session boundaries** — nesting (an agent's session inside a person's),
  whether one may span branches, and what a session id means once a rewrite
  folds its snapshots into a commit.
- **Name/packaging sweep** — `fufu` and binary `ff` against crates.io, npm,
  Homebrew, apt before anything ships. (Known neighbors: `ffuf` the web fuzzer,
  `fuf` an obscure file browser — distinct, but check registries.)
- **Substrate maturity tracking** — which daily verbs must stay binary-backed at
  v1; gitoxide's push support and filter pipeline are the pacing items for
  git-free. (Language/runtime itself is settled: Rust on gitoxide — see
  Substrate.)
