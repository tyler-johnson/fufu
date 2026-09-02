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
rule: fufu's state (the operation log, rewrite map, parked trees, held rewrites) is a *cache
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
same boundary — `ff sync` refuses to publish a stack with held rewrites, while raw
`git push` is git, with the status channel getting loud after the fact rather
than a hook getting in the way.

GUIs and IDEs are therefore first-class writers, not tolerated exceptions. Every
git GUI keeps working identically — showing status, making commits, switching
branches — because fufu's conveniences accrue per-operation to whoever goes
through fufu, and cost nothing to whoever doesn't.

The boundary is execution path, not spelling: the recommended shell alias
(`alias git='ff git'`) moves *typed* git onto the fufu surface — capture-first,
and with `fufu.gitPolicy` deciding what fufu says about a git word it has a verb
for (see `ff git`, below) — while everything that resolves git on PATH stays
foreign.

Automation is not foreign by nature, only by habit. A script, a CI job, or an agent that calls `ff` is inside the surface with everyone else, and gets everything the surface promises. Today they reach for git because fufu gives them nothing better to reach for; the machine surface (below) is the work of making that choice obvious.

## Architecture: three floors

### Floor 1 — Capture (the foundation)

Every working-tree state is continuously snapshotted into refs outside the visible
graph — before commands, at prompts, around any mutation. jj's "the working copy is
a commit" becomes "the working copy *casts* a commit": same guarantee (no state ever
exists only in the filesystem), but HEAD never moves anywhere strange.

Once nothing can be lost, every other layer is allowed to be aggressive. Automatic
history rewriting is only a defensible feature on top of total capture.

Capture is entirely automatic, and there is no verb for asking. Every capture is an operation (see Whole-repo undo), and its description is written by fufu — what ran, or which agent acted — and never by a person, so the log stays a machine's account of what happened rather than a place to leave notes. The manual checkpoint is one of the rituals fufu exists to delete, the way the stash dance is: what a user would reach for by hand already happened before the command they typed. The channels for saying what work *means* are elsewhere and are commits — `ff describe` names the change you are in the middle of, `ff commit` names the one you finished.

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
the working tree *is* the open change, its history is the captures the log
already holds, and no commit exists until the change closes. `ff commit` is the close — build the
tree (`add -A` semantics), write the commit, the branch advances, status is
clean. A clean tree has nothing to close and the close refuses rather than
inventing something: **no empty commit is ever created** — jj's placeholder
commits are exactly the kind of state a boring repository shouldn't contain.
A pending description does not change that; it survives the refusal and waits
for the next close. Descriptions are two-phase: `ff commit -m`
describes the change being closed, `ff start -m` the change being opened — a
pending description parked per branch until its close; bare `ff describe`
edits it, `ff describe <rev>` rewords what's already closed. An undescribed
close is legal ("(no description)", jj-style); hygiene enforces at the exit,
where `ff sync` flags undescribed commits rather than letting them past the
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
forks a branch of your own beginning at their tip and records them as its
base. Their branch is addressed the way git spells it, `origin/feature`: a
tracking ref is what the last fetch left there, and fetching is `ff sync`'s
job and `ff clone`'s, never an address's.

**Branches without ceremony.** Every head is a real ref under `refs/heads/`
from the moment it exists — HEAD never detaches, and branches auto-move as
commits land, because that is git's own behavior once HEAD is attached
(contrast jj's bookmarks, which sit still until told). Work doesn't wait for a
name: every `ff start` mints an **anonymous branch** — a
real branch with a generated name under a reserved prefix (`ff/quiet-lake`) —
unless `-b` names it at birth; `ff describe -b <name>` names it later: a rename that carries the capture
chain, the parked entry, and fufu's metadata along, which is the part a bare
`git branch -m` would orphan. A `-b <name>` flag rides the change verbs
on the same axis as `-m`: on `ff describe` it names the branch you are on,
which is why naming lives there and nowhere else — one verb says what work
*is*, whether the subject is the change's description or the branch's name,
and claiming a petname is not a different act from replacing a chosen one. On `ff start`
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

**Whole-repo undo.** Git has per-ref reflogs but no operation log. fufu has one, and it is the *only* timeline: **every capture is an operation.** A snapshot is not a second concept with its own log and its own ids — it is what an operation carries. An operation records all refs plus the tree state, so `ff undo` restores both together, and there is one address space to learn rather than two. That is the whole reason to merge them: someone asking how to go back should meet one answer, and `ff op` is where it lives. Because fufu is the primary interface the log is near-complete; raw git mutations are tolerated foreign events the capture layer absorbs (reconcile, don't own).

Operations differ only in what they contain. A capture changes no ref — it is the tree alone, taken before every action, at machine rate — while a verb's operation carries ref transitions too, and a *foreign* one records what raw git did behind fufu's back. Kinds sort the log; they do not fork the model. Every operation has a tree, and that is what makes restore uniform: the same thing happens whichever entry you name.

**Undo moves by runs, not by operations.** A capture is a machine's granularity and a person's undo is not, so `ff undo` steps over a *run*: the longest stretch of adjacent captures carrying the same session, ending at the first operation that is not one. The session is only the equality test — no session compares equal to no session — so a run is a fact about adjacency, never a range a tag defines, which is what keeps sessions tags. Only captures group. A verb's operation is a decision somebody made, so it is always its own step and always ends a run — a switch and a commit sharing a session are still two undos — which is also what keeps undo from rolling past a commit by accident. The finer address survives untouched: `ff op restore` still names one capture. What a run collapsed, undo says, because a keystroke that moved forty operations must not have to be inferred.

**Undo moves a pointer; it does not write an operation.** `ff undo` steps this worktree's chain back to the run's predecessor rather than appending an entry saying that it did, so the log records work and never navigation, and undoing an undo is not something anyone has to reason about. What it steps off stays reachable as a branch of the log — that is what `ff redo` walks forward along and what keeps those operations pinned, with the pre-undo capture at its head, so the work you were holding when you undid is the first thing redo hands back. Where the pointer has *been* is recorded where git already keeps such things, in the ref's own reflog: the log answers what happened and the reflog answers where you have stood. New work after an undo forks the log rather than truncating it — nothing is discarded, `ff redo` simply stops offering a path it can no longer take, and the abandoned branch keeps a name `ff op restore` accepts until trim ages it out. `ff op revert` is the opposite half and does write an operation, because inverting one change while later work stands is itself a thing that happened.

Four views, one question each: `ff log` is the commit history, `ff op log` is every operation wearing the ids the `ff op` verbs take, `ff evolog` is the two together with runs collapsed, and `ff history` is where you can go back to — one row per `ff undo` step, with the redo path above `@`. The last two are the ones that render the run: `ff evolog` reads the open change, `ff history` reads the moves. The log answers what happened, and only `ff history` answers what you can do about it, which is why an honest `ff op log` can afford to show everything.

One log, not one per branch: adjacent operations can sit on different branches, and the diff across that seam reads as the whole worktree being replaced. That is literal rather than wrong — switching branches does replace the tree — and it is why reading a single branch's operations is its own walk (`ff evolog`) instead of a filter over neighbors.

Foreign work collapses to a single operation, and should. Git's reflogs record where refs moved but never what the working tree held at each step — that state was never written anywhere and cannot be recovered — so expanding a gap into a reconstructed sequence would manufacture entries with nothing to restore. One gap, one operation, one restore point; the account *inside* it can still be rich, since reflog-derived transitions explain what git did in detail even though undo only lands on the endpoints. Granularity of explanation and granularity of restore are different things, and only the second is bounded by what git left behind. The bound worth saying plainly: a tree change that moves no ref is invisible until the next capture, so `git restore <file>` discards work fufu never saw, and how far back it can take you is set by the last time it was looking.

**Futures-aware status.** `git merge-tree` performs merges entirely in memory, so
"would rebasing onto main conflict?" is a free, side-effect-less query fufu runs
continuously. Status reports futures, not just facts: not "12 commits behind main"
but "main moved; your branch rebases cleanly" or "a rebase would conflict in two
files." The user never has to attempt an operation to learn its cost.

**Branches stack, and a stack is a parent link.** A branch records the *branch* it sits on, not merely the commit it forked from — a commit cannot follow its parent as the parent moves, and following is the whole point. When a parent moves its children go stale, and the same free probe that answers "would rebasing onto main conflict?" answers it one level down: `ff branch` and `ff status` say *parent moved, replays clean* or *parent moved, conflicts in two files*, in the verdicts they already spend. Knowing changes nothing on its own. `ff sync` is what applies it, on the branch you are standing on, and `ff restack` will take the name of one you are not. What makes either safe is that somebody asked by name — not that the branch happened to be underfoot — which is also what keeps propagation from crossing the push boundary by accident, since published history is the one thing fufu will not rewrite behind you.

The cascade is sequential, and saying so is part of the feature: syncing a branch moves it, which makes *its* children stale against a base that did not exist a moment earlier, so their verdicts are recomputed rather than promised. Only the next step's answer is trustworthy, and a whole-tree "all clean" would be a claim fufu cannot honestly make. `ff restack` is the primitive under all of it — replay these commits onto that base, in memory, hold on conflict — and the rest are aims for it: `ff sync` runs it against both things a branch answers to, with the network in front, `ff restack --onto <branch>` records a new parent before replaying, and `ff done` is restack pointed at an edit session's parent, a session being a branch temporarily inserted beneath one. A branch answers to two: the **base** beneath it, and the **remote** copy of itself. Reconciling with either is a replay that can conflict, which is why one verb covers both. Sending is not reconciling, so it is not that verb. Everything `ff sync` does is recorded and reachable from `ff undo`; a push is the one act that leaves the machine, and no operation log reaches across a wire — so `ff publish` is a separate verb, typed on purpose, and there is no knob that makes it happen by itself. Not reaching across a wire is a limit on undo's arm, not on the log's memory: a push is recorded like anything else, and the way back from one is the same verb pointed the other way — undo the commit, publish again, and the lease rolls the shared copy back to where the branch now stands. The lease it carries is the tracking ref as you last saw it, which is also why publish never fetches: refreshing that value first would ask git to guard you against a change you accepted without reading. The exit guards are properties of publishing rather than of a mode, so a held rewrite is refused.

Which side of a divergence is whose is the one question two axes cannot answer on their own: a branch you restacked and a branch a collaborator pushed to are the same shape, and the correct answers are opposite. So sync reads the tracking ref before its fetch and after — **divergence the fetch created is theirs; divergence the operation log accounts for is yours; a tracking tip standing exactly where this branch last published it is yours and undone; anything else is theirs too.** Theirs is incoming, and replays onto the new remote tip; yours is outgoing, and publishes under a lease whose expected value is exactly the tip that did not move — the same fact `--force-with-lease` is built from, spelled once and used twice. The last clause is what keeps the second honest: their commits reach the tracking ref through *any* fetch, so an unmoved ref is silence rather than evidence, and a lease expecting the tip the remote already holds cannot catch the mistake. It is silence only where nothing says who put it there — publish records where it left the shared copy, so a tracking tip equal to that is a tip this repository sent, and what it holds and the branch does not is work of your own you stepped back from rather than work arriving. That memory is a ref rather than a row on the log, because `ff undo` is a pointer move and would rewind the log straight past the one fact undo cannot reverse. Accounted-for means the log recorded the commit as the `old` side of a rewrite or dropped it as empty; anything it does not recognize replays, which never loses work. `--no-fetch` still needs no rule of its own — it reaches the same check.

A restack replays as deep as the branch goes. The futures probe caps itself because it runs at every prompt; a verb somebody typed pays the real cost instead, a cap there refusing a long branch to save an expense nobody was being charged. The working tree enters the account only when the branch it stands on is one the rewrite carries — restacking a branch you are not on is refs and objects, and moves no file at all — and when it is carried, the open change replays as the last step onto the new tip, so a restack that would conflict in uncommitted work refuses before anything moves: the same answer at the same moment, one step further out than the commits. `--onto`'s parent write rides the operation record beside the ref moves, because undoing the replay while leaving the re-aim standing would point a branch at a base it no longer sits on, which is worse than either half alone.

**Every worktree gets its own log.** A linked worktree is another checkout of the same repository: it shares one object store and one ref namespace, and until it also shared one operation chain, one undo pointer and one lock. That is correct for one tree and corrupting for two — a read in a bay appended to the shared chain just by looking, and an undo in the main worktree moved its HEAD to the bay's branch and overwrote its working tree with the bay's, which is a state git itself refuses to create. So the chain is keyed by worktree, at `refs/fufu/wt/<id>/ops`, where `<id>` is the gitdir basename git already files it under and keeps stable across `git worktree move` and `git worktree repair`. A path would survive neither.

The chain lives in the *shared* ref namespace rather than under `refs/worktree/`, and that is a measurement rather than a preference: `git gc` run from the main worktree collects objects pinned only by a linked worktree's `refs/worktree/*`, and reachability is the whole of fufu's gc pin. The same fact is what makes a chain outlive the worktree it belonged to — which is the point, because the case that motivates all of this is an agent deleting a bay with work in it. `git worktree remove` is foreign, so what was never captured is git's to lose; everything fufu had is still addressable, through the same `ff op show` and `ff restore --at-op` as anything else, because the id space is one space across every chain. Retention still reaches those chains on the ordinary `fufu.keep` cadence: surviving the worktree is not the same as living forever. `ff worktree remove` is the one that captures, into the chain of the tree it removes, which is why there is no `--force` and the work survives; ignored files are not carried, and do not come back.

A worktree's chain is parentless rather than forked off another. Rooting it in the main worktree's would make a first-parent walk present another tree's operations as this one's past, and would entangle trim across trees.

**A branch is open in at most one worktree, and fufu is what enforces it.** git refuses to check a branch out twice; gix's ref transactions do not, and fufu moves HEAD through one. So fufu carries the check itself, on every path that opens a branch rather than only on rename and delete, and it names the branch and the worktree the way git does. That is also what makes branch-keyed state safe to leave shared: `refs/fufu/snap/<branch>`, the parked entry, and the branch metadata are all keyed by a name only one tree can hold. What a worktree records in its ref table narrows to match — the refs it owns, plus the tags and the stash stack that are genuinely shared — so an undo moves what this tree is entitled to move and nothing else.

The same rule is what keeps a rewrite honest: a branch carried by a restack can be another worktree's HEAD, so `ff restack` moves the branch it was asked to move and leaves the rest divergent and named. `git rebase --update-refs` carries them, and can, because git's rebase assumes exclusive control of the one worktree running it.

**fufu makes and removes worktrees itself.** The layout it writes is git's own — read off a live worktree rather than inferred — so git accepts it and the two tools stay interchangeable on the same repository. gix has no worktree-creation API, and fufu needed none: the checkout is `write_index_for_tree` and a tree transition from the empty tree, the same two calls every other fufu checkout makes. A worktree path is written into those files the way git writes one — resolved, and spelled with forward slashes on every platform — because git finds a worktree's directory by stripping `/.git` off the end of its `gitdir` file, and a Windows path spelled natively does not end in `/.git`: the strip fails silently and git reports the admin file where the worktree belongs. Everywhere else fufu records, prints, and compares a worktree path through one function, and compares by identity rather than by spelling, since the path a person types and the path fufu resolved differ on any machine whose temporary directory is reached through a symlink. Creation and removal are both undoable, and undoing a creation captures before it deletes — the only effect fufu records whose inverse touches the filesystem outside `.git`, and capture-first is what makes it acceptable.

### Floor 3 — Rewrite

**Stable change identity.** A rewrite map (old-sha → new-sha, maintained in refs)
lets descendants follow a *change* across amends and rebases, the way jj's change
IDs do. Git's own machinery has been converging on the needed primitives for years —
`merge-tree`, `rebase --update-refs`, `rerere`, autosquash. fufu wires them into an
autopilot.

The map lives in the operation record, as a field on the op rather than in refs of its own. The log is already the authority for what happened and already pins the old commits, so undo and `ff trim` cover the map for free and nothing else has to learn it exists. A lookup index over it waits for a reader — the first is revalidating a held rewrite — and when one arrives it materializes the way the op-id index already does.

Re-parenting a merge and replaying one are not the same act. A rewrite that moves no tree — a reword — re-parents merges along with everything else, since parents are precisely what re-parenting knows how to fix. A rewrite that moves a tree has to replay, and what a replay means for a merge is the ambiguity the futures probe already declines to answer, so the rewrite declines it too rather than picking a side nobody asked for.

**No empty commit survives a replay.** A commit whose replayed tree matches its new first parent's introduces nothing, so it is not written: its descendants and the branches sitting on it follow to that parent, which the rewrite map already knows how to do. git keeps a commit that started empty and drops one a rebase emptied; fufu drops both, on the same rule that stops `ff commit` from closing a clean tree — empty is empty, whatever emptied it, and a description worth keeping is still in the operation log where undo would find it. A merge is never dropped, because collapsing one onto its first parent would erase the other side of the history, and neither is a root, which has no parent to collapse onto. A reword drops nothing at all: it re-parents rather than replays, so every tree it passes over is one it did not touch. Every rewrite says what it dropped — a deliberate empty marker is someone's, and removing it in silence is the exact thing *deferred requires loud* exists to prevent.

**Nothing guards a rewrite of published commits.** Every exit guard sits on the exit: `ff sync` refuses to publish a held rewrite, and raw `git push` is git. A second boundary on the rewrite verb would refuse the most common real case there is — fixing the message on a branch you have already pushed — so the rewrite proceeds and says so, noting when the commits it rewrote are still reachable from the branch's remote. Disclosure rather than obstruction, and the force-with-lease question stays where it belongs, on `ff sync`.

**Mid-stack editing, two reaches.** jj edits a mid-stack commit by traveling to
it (`jj edit`), which in git terms means detaching HEAD. fufu keeps the reach and
drops the detachment. The short reach is at a distance: `ff absorb` applies working changes to a commit in memory — `HEAD` unless `--into` aims another — restacks its descendants in memory, and moves refs; you never leave your tip. `ff lift` is the same reach run backwards, taking changes out of a commit and into the open change, and it moves no files at all: the change stays applied and merely stops being committed, so the tip tree loses exactly what the open change gains. A descendant that depends on a lifted change cannot replay without it, and holds like any other rewrite.

Neither verb guesses. The commit is named or it is `HEAD`. Attributing each hunk to the commit it belongs to — `hg absorb`'s trick, and the reason the verb carries that name — is a later integration rather than the default behavior, because a verb that guesses wrong at a distance is worse than one that asks. Lifting a commit's every change drops the commit, which is not the same kind of guess: what to attribute a hunk to is a question about intent, and whether a commit changes anything is a fact about its tree. The long reach
is a session, and a session is a branch: `ff edit <rev>` mints an anonymous branch there and switches to it, leaving the branch you came from exactly where it stands. You edit the commit's actual content — the thing distance can't give you — and `ff done` amends it, replays the commits that were ahead, and lands you back. A conflicting replay holds like any other rewrite. Your open change parks and returns by the rules leaving and arriving already have, because a session *is* a switch — it inherits the parked-work machinery rather than needing any of its own.

Travel happens in ref-space, and HEAD and the working tree agree at every moment, which is the whole reason to do it this way: plain git sees an ordinary branch with ordinary edits, the capture floor records ordinary trees, and no verb has to ask whether a session is running. Holding the old tree against a newer HEAD would instead make every session look to git like a wholesale revert, and would suspend the tree-agrees-with-HEAD invariant the rest of fufu is built on. The commits left ahead stay on the branch that already held them, so they need no home of their own and no new grammar to name — they are `@..<branch>`, which is what `ff log` dims above the open change while a session runs. And sessions are resumable because they are branches: the state is one field of the anonymous branch's metadata naming the branch to replay onto, switching away and coming back costs nothing, abandoning one is deleting the branch, and a session branch says so wherever branches are listed, because an unfinished one is worth noticing.

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
the session's edits stay in the log regardless.

Ending one is a single operation, which is a rule rather than an implementation detail: the amend, the replay, the branches the replay carries, HEAD's return and the parked change coming back all land together, so one `ff undo` takes the whole session back. Two operations would not have halved the cost of undoing it, they would have doubled it — every verb writes its plan ahead, so the state an operation records is the world *after* it, and a first undo would land inside a half-finished session rather than beside it. Until the automation above exists there is nowhere for a conflicting one to stop, so the interim is the one held rewrites already take: `ff commit` inside a session refuses and names `ff done` — which is what it would have collapsed to anyway, since mid-session the open change *is* the edit and landing it leaves nothing to commit. A session branch that has gained a commit of its own is refused rather than folded, for the reason the floor turns on: its content would survive the amend but its message would not, and discarding a message is exactly the guess the verb had just declined to make.

Because a session *is* an ordinary branch, every verb that already exists reaches it, and the rule is whether the verb would move it off the commit being edited. `ff absorb`, `ff lift` and `ff describe` rewrite that commit in place, so they are simply carried: what the session lands is whatever the working tree holds *and* whatever the commit now says, since an amend that changed content and message is one act and splitting it would land half. `ff restack` would move the branch to a different commit entirely, so it refuses. And a session's base axis stays silent for the reason unmerged work's does — sitting below the branch it will land on is what a session *is*, not a chore waiting to be done, and a status line urging you to close the gap would be urging you to destroy the session.

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
result, unresolved regions carried forward as literal marker content, and a
commit whose own changes land clear of the marks replaying over them untouched.
jj's conflicts also *simplify* as they propagate, because they are expressions;
fufu's are text, and text does not simplify itself — what a later commit can do
is leave a mark alone, not dissolve it. `ff resolve` then presents every
standing region in the working tree in one editing session — ordinary conflict
markers, both sides labeled with the step that wrote them
(`>>>>>>> rebasing "add parser options" (3/10)`), the incoming side carrying the
commit because that is where git puts it and therefore where a reader already
looks. Those labels are not decoration: they are the only thing that survives
into the tree saying which step a region belongs to, and they are what the
landing attributes each edit against. `ff done` ends the session:
resolutions are absorbed back into their owning steps (the `ff absorb` machinery
pointed at the replay), the chain re-runs in memory, and the whole rebased stack
lands at once: refs move
one time, every landed commit clean, no conflicted state ever existing in the
graph. When two commits conflict on the same region the carried markers do not
nest, they interleave — the earlier block stops bracketing anything — which is
jj's notorious ergonomic wart wearing git's clothes. So the chain stops rather
than write one: `ff resolve` presents the steps before the tangle, and what is
left is held again. A stack of tangles unwinds one round at a time, without
anyone having to know the word. `ff resolve --step` keeps the sequential
per-commit mode for when that is the shape you want anyway.

What a hold records is the verb's own question — the branch, the target, what it
was asked to become — and never the plan it could not finish computing. That is
what makes revalidation a recomputation rather than a comparison: nothing has to
be pinned, because every input is a ref or the working tree; the pending rewrite
replaying over whatever you add costs nothing, because the replan sees what you
added; and a target that has gone, or moved out of history, expires the hold at
the moment somebody asks rather than at some sweep that has to be scheduled.

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
3. **Exits blocked** — `ff sync` refuses to publish anything with a held rewrite,
   the way jj refuses to push conflicted commits. (The guard lives on the fufu
   surface: raw `git push` is git — the two regimes — and reconciliation gets
   loud after the fact instead of a hook getting in the way.)

Deferred and quiet is how work rots; deferred and loud is the whole trick.

A hold is a thing that happened, so it reports like one: the verb succeeds, says what conflicts and where, and exits 3 — the code that already means nothing was touched and a human decision is required. The third discipline sits on `ff publish` and nowhere else: a hold refuses the push and stops nothing local, because a rewrite that cannot leave the machine is still one you can keep working on.

## Command surface

fufu is the daily driver; git is the escape hatch for genuinely advanced work
(bisect, submodules, plumbing, forensics). "Advanced" means *rare*, not *dangerous* —
the dangerous-but-daily git commands (`rebase -i`, `stash`, `reset`, reflog
spelunking) are exactly what fufu's verbs replace.

**The rule that keeps this honest: every fufu verb must earn its existence by doing
something git's version doesn't** — routing through the operation log, enforcing an
exit guard, engaging tree memory, capturing first. If a proposed verb would behave
identically to its git counterpart, it must not exist; git is right there. The
moment fufu verbs are just spellings, the tool collapses into shell aliases with
extra steps.

The daily surface. Where the workflow is jj's, the vocabulary is too: `edit`
and `describe` are deliberate imports, and jj's `new` survives as the alias for
`ff start`, with `switch` staying underneath them as the general movement verb.

| verb | what it does | what it replaces |
|---|---|---|
| `ff init [<dir>]` | start a repository with the net already on: the gc guard written and the operation log's floor taken before you type anything else. Inside a repository that exists it means *turn fufu on here*, and says so | `git init`, then hoping something later remembers to arm it |
| `ff clone <url> [<dir>]` | that, plus fufu owning the sequence: it speaks the protocol itself, resolves the remote's HEAD, checks out the worktree, and reports in fufu's vocabulary (`247 commits on main`). Also where the shared-copy memory `ff publish` reads starts out true | `git clone`, and a repository with nothing for `ff undo` to land on |
| `ff` (spelled out, `ff map`) | the map: recent work across every branch, parked changes included — where you left things | `git branch -v`, `git stash list`, and remembering |
| `ff status` | `ff log` cropped to two rows — the open change and the commit under it — with the diffstat between them, plus futures: held rewrites, "rebases cleanly onto main" | `git status` + attempting things to see if they work |
| `ff diff [<paths>]` | the open change as a patch — the same tree diff `ff status` counts, read down to the line, so the untracked files `git diff` is blind to arrive with their content. The body is git's unified diff verbatim: a patch format is not fufu's to invent, and what comes out of here is what `git apply` takes | `git diff`, which cannot see the half of the change that is not tracked yet |
| `ff show [<rev>] [<paths>]` | one revision with its patch: the commit's furniture, then what it did against its first parent. Bare it is `@`, the open change, printing exactly `ff diff`'s body — one renderer for the thing you are about to commit and the thing you committed last. A merge names the ambiguity rather than picking a parent for you | `git show`, which has no `@` to point at |
| `ff commit [<paths>]` | close the open change: commit the working tree (`-m` describes what's closing, `-b` names where it lands — claims a placeholder, else a new branch); `<paths>` closes a slice and leaves the rest open, and the interactive form picks hunks — a slice cut from the stream, chosen at the close rather than kept between closes | the `add`/index two-phase ritual (which still works, for those who want it) |
| `ff describe [<rev>] [-m <msg>] [-b <name>]` | reword any commit's message (`-m` inline, else the editor) — bare form edits the open change's pending description; `-b` names the branch you are on, petname or chosen alike, and is the only verb that does; descendants restack in memory | `commit --amend` at the tip, `rebase -i` reword dances anywhere deeper |
| `ff start [<rev>] [-m <msg>] [-b <name>]` (alias `ff new`) | begin new work on a fresh branch, always: bare forks trunk, a `<rev>` forks there; the open change parks and the new branch opens clean; `-m` describes the change being *opened*, `-b` names the minted branch (else anonymous); never an empty commit | `git switch -c` + the stash dance |
| `ff switch <branch>` | branch switch with tree memory | `stash` dances |
| `ff branch <list\|delete>` | the bookkeeping left over once naming lives on `ff describe -b`: what exists, and taking one away — recorded, undoable, parked-entry-aware. A published branch's copy on the remote is not the name's to take: the delete says it is still there, and `--shared` is how you say remove that too — leased, and the one half `ff undo` cannot reach | `git branch` bookkeeping |
| `ff worktree <add\|remove\|list>` | the worktrees this repository has, and the chains of the ones that are gone: the chain floor is laid as the worktree is made, so `ff undo` works there from the first command; the removal captures into the removed tree's own chain before the tree goes, which is why there is no `--force` and the work survives; the listing shows chains whose worktree is gone, which git cannot know about | `git worktree add`, and losing whatever was uncommitted when a tree went away |
| `ff absorb [<paths>]` | fold working changes into a past commit — `HEAD`, or `--into <rev>` — and restack its descendants in memory | `commit --fixup` + `rebase -i --autosquash` |
| `ff lift [<paths>]` | the counterpart: take changes back out of a past commit (`HEAD`, or `--from <rev>`) into the open change, restacking descendants. Only ownership moves; no file does | nothing |
| `ff edit <rev>` | editing session on any commit: mints an anonymous branch there and switches to it. The branch you came from stays put and its commits wait ahead; given a branch name it simply is `ff switch` | detached-HEAD `rebase -i` edit dances |
| `ff prev` / `ff next` | scrub one commit back or forward. `prev` opens a session — the first one from the tip is editing `HEAD` — and `next` replays the commit waiting ahead, which makes `ff done` exactly `ff next` until nothing is | `rebase -i` reword/edit dances |
| `ff done` | finish the current session (`edit` or `resolve`): absorb the edits, restack in memory, land, return to tip | `rebase --continue` ceremony |
| `ff collide [<a>] <b>` | the sideways axis: would these two branches hit each other if both landed? A read, replayed in memory against the merge base, so the answer costs nothing and changes nothing. One name means the branch you are on against that one. Each side is judged on the tree the operation log holds, so a branch checked out in another worktree — or nowhere — still answers, uncommitted work and all. One pair is the whole verb: which sets of branches can fly together is scheduling, and scheduling needs a queue and something to claim with, neither of which fufu has | attempting the merge to find out, then undoing it |
| `ff restack [<branch>] [--onto <base>]` | the primitive, offline: replay a branch's commits onto the branch it sits on — yours unless you name another; `--onto` records a new parent first, which is how a branch is re-aimed. It moves the branch you named and no other: descendants inside the range are left where they stand and said so, because a carried ref can be another worktree's HEAD | `rebase --onto` arithmetic, and `--update-refs` moving branches you were not thinking about |
| `ff sync [--no-fetch]` | line this branch up with both things it answers to — restack onto the base if it moved, reconcile with the remote: land if clean, hold if not. Nothing leaves the machine; it names what is left to publish | `fetch` + `rebase`, in the right order, and prayer |
| `ff publish [--to <remote>]` | send this branch to its remote under a lease — creating the shared copy, or putting back one that was deleted. `--to` names the remote for a branch that answers to none yet, and records it, so the ambiguity is settled once rather than at every verb. The one act fufu cannot undo, so it is the one you type | `push --force-with-lease`, and remembering which of `--force` and `--force-with-lease` you meant |
| `ff remote` | what the remotes here are called, and where each points. A read, and the reason it exists is that fufu's own verbs name a remote — `ff publish --to` checks the name, `ff sync` refuses to guess between several — so the list they check against should not have to be borrowed from another tool. Adding one is still `ff git remote add`: a name and a URL are two facts, and fufu has no verb that takes them | `git remote -v`, which is where fufu's own refusals used to send you |
| `ff undo` | step the whole repository back one run — refs and tree together, and a run of captures is one step. Takes no argument, and repeats: each one goes further back | reflog archaeology, `reset --hard` fear |
| `ff redo` | the complement, moving forward again after one or more undos | nothing |
| `ff op <log\|show\|diff\|restore\|revert>` | the operation log as objects: read every operation on it (the argument is the set language, and narrowing is its job — there is no flag), show what one changed, compare two, rewind the repository to one (`restore`), or invert a single one and leave later work standing (`revert`). Deleting operations is `ff trim`'s job alone | nothing — git has no operation log |
| `ff log [-r <revset>] [<paths>]` | changes as the spine, jj-style: the open change (`@`) atop the commit walk (`●`), each commit wearing its newest operation's id; `-r` takes the set language and the positional takes paths, so nothing needs a `--` to tell them apart. A path follows its renames — `-r` filters without following, because a set has no line of descent to carry a name along | `reflog` + `log` |
| `ff evolog` | commits and the operations between them, newest first, runs collapsed the way `ff undo` moves — the combined view, and the drill-in behind `ff log`'s letters column | `reflog` spelunking |
| `ff history` | where you can go back to: one row per `ff undo` step, the redo path above `@`, and a run of captures collapsed into the single row it undoes as, saying how many | counting reflog entries and hoping |
| `ff restore <path> [--from <rev>]` | pull paths back: bare, from the commit under the open change; `--from` names another revision; `--at-op` reaches a past operation, `--at` the operation that was current at a given time | hoping |
| `ff trim` | drop operations past the keep window — trash-first, so the last trim is itself undoable; rides an ff command daily, so retention enforces itself | remembering to prune, or quietly never pruning |
| `ff resolve` | all of a held rewrite's conflicts, one editing session, on your schedule | sequential stop-fix-continue rebasing |
| `ff git <args>` | capture-first passthrough, verbatim always; `fufu.gitPolicy` graduates what fufu *says* about a git word it has a verb for — observe, coach, strict | raw git without a net |
| `ff config` | every setting in one place: typed registry, defaults on display, values validated before they land | `git config` guesswork and doc-spelunking |
| `ff version` | which fufu this is: the release, the commit it was built from, and — read from the update lane's cache, without touching the network — whether it is still the current one. `--json` reports the three as fields | `git version`, and a separate trip to find out you are behind |
| `ff update` | move this binary to the latest release: verified download, atomic swap; a passive lane checks ~daily and auto-installs, or prints a one-line notice | re-running installers, stale binaries |
| `ff doctor` | verify the net: chains, identity, reflogs, gc guard, objects, the remote floor, wiring, update — `--fix` repairs the gc keys and config left naming a branch gone from both sides | "is this thing even on?" doubt |
| `ff explain <id>` | what an error id means, and the ways out of it | searching the message text |
| `ff watch` | stream operations as they land, newline-delimited JSON; `--all` is every worktree in the repository on one stream, each line naming the tree it came from | polling `git status` in a loop, or one process per worktree |
| `ff completions <shell>` | shell completion, with branches, revs, and op ids resolved live | hand-rolled dotfile fragments |

Two flag conventions keep the surface from turning into a scramble for letters. **`-m` describes what the verb creates** — the commit `ff commit` closes, the change `ff start` opens, the description `ff describe` sets — so one letter means one thing across every verb that makes something. And **shared flags are long-only**: `--json` and `--session` ride every verb — one because every verb has output, the other because every verb captures first — while `--at-op` and `--at` ride every verb that *reads*. None of them takes a short form, so a verb may claim any letter without consulting a list, and the collision is caught by the parser rather than by convention. Two short letters are reserved above the verbs. `-v` is the version — lowercase, because fufu has no verbose flag to hold the letter for, and `-V` is a shift away from nothing. It is declared on the root alone rather than globally, so verbs stay free to claim `v` for themselves; `-V` is kept only to answer the habit, on the same rule as the retired `-m`. `-C <dir>` is the second, and it is git's spelling bought back from the long-only rule: the habit is already in everybody's fingers, uppercase was spoken for by `-V` anyway, and no verb claims an uppercase letter. `--cwd` is its canonical long name, on the same arrangement `-v`/`--version` has. It runs the whole command as if fufu had been started in `<dir>` — a chdir, so a relative path argument after it resolves there too, which is git's semantic and the reason the flag is not called `--repo`. Repeating it is refused rather than ranked — clap's default, and the one `--session` already has — which is where fufu stops following git: git accumulates repeated `-C`, each resolved against the last.

`--at-op <op>` resolves the whole command against the repository as it stood at that operation: `ff --at-op <op> status` prints what `ff status` would have printed then, and `ff --at-op <op> restore <path>` pulls that path back from there. `--at <time>` is that same reach addressed by the clock — the operation current at that moment, resolved once and then indistinguishable from having named it — so the two are one mechanism with two doors, and naming both at once is an error. Two flags rather than one is what holds each to a single kind: an id is never a date, and a date is never an id.

Both ride every verb that reads repository state, and no others. The line is not read-versus-write — `ff restore` writes files and takes them happily, since what it reads is its source — but whether the verb has an input state to place at all: `ff commit` and `ff start` only add to now, so a past operation has nothing to say to them. `--at-op` is also the *only* way an operation id enters a command outside the `ff op` family, which is what keeps the two address spaces from bleeding (see One target grammar).

Two spellings are not two verbs. Seven verbs take a short one as well — `st`, `ci`, `sw`, `br`, `ev`, `desc`, `cfg` — and the set is curated and closed rather than derived: prefix inference would make the accepted spellings a function of the verb list, so `ff sta` would work until the day a verb starting with the same three letters shipped, and a spelling that stops working is worse than one that never worked. Short forms stay out of the command list, which is what fufu *does* rather than how it is typed, and the root page teaches them instead. Bare `ff` also answers to `ff map`, so the word every page uses for it is a word you can type, and the map gets a help page of its own address.

A handful of git words are answered rather than parsed: `checkout` (and `co`), `diff`, `stash`, `pull`, `push`, `rebase`, `merge`, `blame`, `tag`. Typing one is a question — how do I do the git thing here? — and "unrecognized subcommand" answers a different one, so each raises a coded refusal naming the verbs that replaced it, with what was typed folded into the exits the way `ff branch <name>` folds it. This is the retired `-m` and `ff log --ops` again: a word fufu chose not to have is worth more than a word it never heard of. Two shapes qualify, and `rebase` and `merge` are one of each: a verb fufu has not written yet, and a verb it declined to have — principle 12 takes rebase over merge, so replaying is the answer and not a placeholder. The rest earn the entry on the half git does not answer: `blame` reads history, and the work fufu is holding is the part that is not history yet; a tag is git's to make, but `refs/tags/` rides every operation's ref table, so putting back one that was deleted is `ff undo`. They are refusals and nothing else, so they capture nothing — a snapshot taken for a command that does not exist would be a row on the log for something that never ran.

### `ff init` and `ff clone`

The earn is the same for both, and it is the two things `git init` and `git clone` leave undone: the gc guard is written before anything can expire, and the operation log's floor is laid so `ff undo` has somewhere to land from the first command onward rather than from whenever an ff verb first happened to take one. `ff clone` adds fufu owning the sequence — its own protocol, its own checkout, its own report — which is also the moment the shared-copy memory `ff publish` reads starts out true instead of inferred.

Run inside a repository that already exists, `ff init` means *turn fufu on here*: the same work, reported honestly as already done when it was. That is the adopt path for a repository git made, and it is why the verb is not `ff init` in the git sense at all — a `--bare` is refused, naming `ff git init --bare`, because a bare repository has no working tree and therefore nothing for a floor to hold.

These are the only two verbs that capture **last**. Every other verb captures first; these cannot, because there is no repository to capture until the work is done. What they take is not a pre-command snapshot but the log's first entry, and an append before the clone happened would be a claim nothing could falsify — the same shape `ff publish`'s record takes, for the same reason.

Neither pesters about hooks. Every agent client's config file and every shell rc file belongs to the user, not to this repository; `ff hook` wires them when asked and `ff doctor` already owns the report.

### Presentation conventions

Operation ids are spelled in jj's reverse-hex alphabet: hex digit value `i` maps to `"zyxwvutsrqponmlk"[i]`, so `0` → `z` down through `f` → `k`. The letter range k–z shares no character with hex, so an op id can never be misread as a commit sha — which matters more here than it looks, since an operation *is* a commit and its raw hex would be indistinguishable from any other. The letters are what make the two address spaces visibly different at a glance, so they are the only spelling — displayed everywhere an op appears, and the only form accepted where one is input (`ff op <verb>`, `--at-op`). Raw hex is not a second way to say the same thing; it is how you say *commit*.

Op id columns highlight the shortest unique prefix: bold what you can type, dim the rest. The uniqueness domain is exactly the set `ff op` resolves against — the operation log, live and trashed — so the bold prefix is precisely what those verbs accept unambiguously. That domain is materialized under `<common-dir>/fufu/ids/`, appended by capture and rebuilt whenever the log tip moves out from under it, so highlighting and resolution read one list rather than two code paths agreeing. Commit shas get no highlighting: they display as a plain eight characters, one fixed width rather than a probe (eight is effectively always odb-unique at this repo's scale, git resolves any rare ambiguity when one is pasted, and a column that renarrows as the repository grows stops lining up); the op column is where the highlighting pays. One palette serves the whole tool, and it colors **roles, not commands**: op ids magenta, commit shas blue, ages cyan, the working-copy `@` green, insertions green and deletions red, rails and asides dim, plus three verdict colors — green for clear, orange for trouble, blue for commits pending against the remote, either direction. Prose stays plain; color marks the tokens you scan for (hex, ids, times) and the verdicts you act on. That is why `ff doctor`'s WARN, a rebase that would conflict, and a check that failed are all the same orange: one color per meaning, wherever the meaning turns up. Color is redundant encoding and never the only encoding: every verdict carries a word or a glyph saying the same thing, so `NO_COLOR`, a monochrome terminal, and a screen reader cost the reader decoration and never information.

`ff status` is that same picture cropped to two rows: `@`, the open change, and the commit under it, with the diff between them hanging on the rail that joins them — one line per file: a change-kind letter, the path, insertions and deletions counted separately, and a width-scaled `+`/`-` bar. The two counts stay apart rather than summing to git's single total because "18 changed" is the one number nobody acts on, and the letter earns its column because no bar can tell a new 40-line file from one that grew by 40, while binary and mode changes have no numbers to draw at all. There are no sections: a file is ignored or it is listed. Conflicts keep a block of their own — that is a state, not a staging distinction. The header closes with the sync line, built from two nouns a person learns once: the **base** is what this work sits on (trunk, or the branch it was stacked on), the **remote** is the shared copy of this same branch. Both are restacks — something you sit on moved; would replaying onto it be clean? — so both spend one vocabulary and the same verdict colors: green when it replays clean, orange when it conflicts, blue for commits pending against the remote either way, dim when there is nothing to say. The line says roles, not branch names; a name appears only when the role resolves to something a reader would not have guessed — a base that is not trunk, a remote that is not this branch's own — because that is exactly when the name is news and every other time it is noise. Ref syntax never appears: `origin/feature` is a cache of what a remote held at last fetch wearing a branch's name, and making a person reconcile it by hand is the confusion fufu exists to delete.

What the line reports is what `ff sync` would do, which is also what decides whether it speaks at all: an axis stays silent when sync would not act on it, and when neither would, a single dim `nothing to sync` stands for both — never "in sync", which a reader can hear as "merged". So being ahead of your base is silent while being ahead of your remote is not: sync never merges you into your base, making unmerged work a branch's permanent condition rather than pending work, while unpushed commits are precisely what sync will send. The palette is 256-color and deliberately desaturated: the base sixteen have no orange at all, and saturated glyphs make the dim rails beside them look broken — anstream downgrades on terminals that can't do 256. Which palette is `fufu.theme`: `muted` by default, `vivid` for the saturated cut, and `terminal` to drop to the base sixteen and inherit whatever the user's own terminal theme defines — the one honest choice for someone who has already tuned their colors and wants every tool to respect them. `--json` carries the model rather than the crop (see The machine surface): the same `changes` array, plus `open`, `parent`, and the futures the header compresses into one line.

Bare `ff` spreads that same row grammar over the whole repository. The map is a **skeleton**, not a budget, and the skeleton is *relational*: only the commits that relate the shown branches are drawn — their tips, the joins where one branch's history parts from another's, the merges that land a shown branch — and every run between them contracts to one `~ N commits` row. Structure that relates only vanished history earns no row: a merged-and-deleted branch's merge commit and fork point are the trunk's own past, not a relation between anything on the map, and on a merge-heavy trunk they are nearly every commit, which is what makes graph tools that draw them unreadable there. The walk already knows the difference for free — it tracks which shown tips reach each commit, so a join is where two reach-sets meet, and a fork with two children but one reach-set is no join at all. Elision counts read along the first-parent line, straight through the invisible merges, so `~ N` means N commits along *this* line; what a vanished merge brought in is gone, not counted. jj's `~` says only that there is more; fufu's says how much, and a bare `~` means the walk stopped with history still below it. A run of exactly one commit is drawn rather than elided: an elision row that saves no lines and tells you less is a bad trade. The glyph set and the curved rails are jj's, and so is the two-line row — the node line carries the payload, the edge line carries the subject *and* the lane transitions. Branch names are what the map exists to help you find, so they are called out four ways at once — `▸ [name]`, underlined, bold — and not one of them is a color. There is none left to spend: magenta, blue, cyan and green are all already on a map row, and the only unused roles mean *trouble*, so a branch name wearing one would read as a problem. Emphasis is therefore shape and modifiers. Bold says what it already says on an op id's shortest unique prefix — this is what you can type — and a branch name is the other typeable token on a row, since `ff switch` takes it; the underline says jump target, which is the same thing in the register every other tool uses it in; the sigil and the brackets are pure shape, which is the point, because they survive a pipe, `NO_COLOR`, a monochrome terminal and a screen reader, and emphasis carried by color alone would not. The current branch adds the `@` green over all of it: the rest reads "you could go here", the green reads "you are here". An op id column with nothing in it is one dim em dash rather than blank, so the sha beside it reads as the second column instead of as indentation. Branches are ranked by tip time and bounded, with trunk and the current branch always present so the picture has a floor and a you-are-here; the walk itself is bounded by `fufu.mapDepth`. What the map does not carry is verdicts: `ff status` owns "rebases cleanly", and the most-typed command in the tool must not pay a merge simulation per branch.

`ff branch list` is that same row grammar laid out as a table: the map's label character for character, and `@` for where you are standing rather than git's `*`, since a listing that spelled you-are-here differently from every other surface would be teaching two things at once. What a row has to say hangs on a second indented line in `ff status`'s two nouns — through the same renderer, not a second wording kept in step by hand — so a verdict can never trail off the right edge behind a long subject, which is exactly what the old `[main: conflicts]` did. The remote half is the cheap local counts rather than a probe, because the rule that keeps verdicts off the map keeps a merge simulation off every row here too; the price is an aliased remote losing the name it carries on the status line, which is the rare case rather than the daily one. Below the local branches sits a third section, the branches a remote holds that no local branch tracks — subtracted by tracking ref rather than by name, so a branch tracking somebody else's is not counted twice. This is the listing's job and not the map's: the map is a relational skeleton of *your* work and the most-typed command must not pay a per-branch cost, while a verb whose stated question is "what exists" that answers only "what is local" is answering a smaller question than it was asked. Those rows spell the name `ff start` takes and wear the sigil without the brackets — the one place this listing departs from the map's label, because the brackets say *this is a name you can type at `ff switch`* and `ff switch` resolves local names only. Withholding two characters is cheaper than promising the wrong verb. The section is ranked by tip time and bounded like the map's branches, with the remainder standing as one dim `~ N more` — the map's own elision grammar — and `--all` unbounds it; the count is on the model rather than in the rendering, so a machine reader is told what it did not get.

The log family (`ff log`, `ff evolog`, `ff op log`) pages on a TTY, git-style: `fufu.pager` config, then `FF_PAGER`, then `PAGER`, then `less`, whitespace-split with no shell quoting. `LESS=FR` and `LESSCHARSET=utf-8` are provided when unset (quit if one screen, keep ANSI colors). git's default adds `X`, which suppresses the terminal init string so less never enters the alternate screen; a terminal only routes wheel events to a program that is on it, so `-X` costs mouse scrolling, and what it worked around was fixed in less 530. Piped output and `--json` never page; a pager that fails to spawn falls back to direct printing, silently. Color follows anstream's auto-detection — `NO_COLOR`, `TERM=dumb`, and non-TTY stdout all disable it, and the decision is made against the real terminal before the pager pipe wraps it. No `--color` flag yet; the knobs that exist are the ambient ones.

The root page's command list is grouped under lowercase headings rather than printed flat, and the headings are `git help`'s own words wherever they fit — start a working area, work on the current change, examine the history and state, grow, mark and tweak your common history, collaborate — with the fufu-only groups (go back; wire it in, and check on it; fufu itself) written in the same register. A reader who knows git's page should not have to learn a second one, and forty commands in one alphabetically-arbitrary block is a page people scan instead of read. `ff -h` shows fourteen common verbs under six headings and closes with a dim line naming `ff --help`, which shows all forty under eight — git's `git help` / `git help -a` split, spelled in the flags clap already owns. Three placements depart from git: `commit` sits with the current change rather than under grow-mark-and-tweak, because here the working tree *is* the change; `restore` sits there too, where git has it; and `map` heads examine rather than taking a line of its own, since bare `ff` is taught in the prose above the list. clap cannot group subcommands, so fufu renders the block itself and hands it to clap as a `help_template` whose `{options}` — in place of `{all-args}` — is what keeps clap's flat list from rendering at all. Nothing is hidden to achieve it, so suggestions, `ff help <command>` and dispatch are untouched, and every command's own page stays clap's.

### `ff git` and the alias

Like jog, fufu ships a passthrough and recommends the shell alias
(`alias git='ff git'`): typed git snapshots first, then runs, verbatim. That
is the whole contract, under every setting: fufu never runs a write verb you
did not type. Translation — swapping one command for another behind the
person who typed it — was the one thing a correction mechanism must not do,
and it is gone rather than re-gated.

What is graduated instead is what fufu *says*. `fufu.gitPolicy` has three
tiers and governs both entry points at once, because both exist: `ff git` is
what a person types through the alias, and a raw `git …` inside an agent's
shell tool is what the `PreToolUse` hook sees. **observe** captures and
records and says nothing. **coach**, the default, names the fufu alternative
the first time a given git word comes up — `tip: that's ff commit` on the
alias, the same sentence injected into the model's context through the hook.
**strict** refuses the unambiguous ones outright: exit 2 on the alias, a
`permissionDecision: deny` through the hook, each naming the verb to run.

Three rules make that safe, and each is load-bearing. fufu never runs a write
verb you did not type. fufu only corrects what it can answer, so the set is
exactly the git words that have a fufu verb to name — `git apply`, `git am`,
`git bisect`, `git submodule`, `git gc` are writes with no fufu answer and are
never touched, which makes the table self-limiting and keeps `ff git <args…>`
an honest escape hatch even under strict. And ambiguity fails open: a shell
string that is not one plain `git <word> …` invocation is never denied and
never coached, because shell parsing and compound commands make guessing on
somebody else's command line exactly the wrong risk to take. The capture
already happened, so the net is intact either way.

The regime boundary is execution path, not spelling: through the alias, git
spellings *are* the fufu surface, while scripts, IDEs, and GUIs resolve real
git on PATH and stay foreign — the same scoping as jog's alias. Coaching is
policy, not nag: once per word per session, with the alias counting as one
long session called local, so a person at a shell hears each word once per
repository. A refusal is not throttled, because a refusal is the answer
rather than a nudge. `ff doctor` reports how often the lane fired.

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

Retention with an undo. `ff trim` drops the oldest suffix of the operation log past `fufu.keep` (90 days by default). The pre-trim tip is written to trash before a single ref moves, so the last trim is itself recoverable; survivors keep their trees, messages, and dates byte-for-byte, only parent slots relink, and the reflog is replayed with the original times so `@{n}` and `@{time}` stay truthful. A crash mid-trim leaves a shorter-but-valid chain and the full pre-trim state in trash.

The earned existence is the automatic half: a safety net whose upkeep is a chore is a net that quietly rots. A trim rides an ff command at most once per `fufu.autoTrim` (daily by default), per repository, and it runs **inline** — the engine is native, so there is no child to spawn and nothing to wait on. The hot path is one read of a per-repo stamp beside the common git dir; config is consulted only when the stamp says a trim might be due, and the stamp is written *before* the trim runs, so a failure retries on the cadence rather than on every command. The one thing the automatic lane deliberately skips is manual trim's `git gc --auto` nudge: that would put a spawn on the commands that carry the lane, and bare `ff`, `ff git`, and `ff trigger` stay provably spawn-free. A hand-run `ff trim` nudges whether or not it dropped anything: native writes never trigger auto-gc, so without that nudge nothing ever packs the store, and an unpacked store taxes every chain walk long before retention is due. `gc --auto` is self-limiting, so the nudge costs nothing until git itself thinks packing is worthwhile. `fufu.autoTrim false` leaves trimming entirely by hand.

### `ff config`

jog's config command, carried over: no subcommands, arity decides. Bare `ff config` lists every setting with its value, its meaning, and a `(default)` marker; a key gets; key plus value sets; `--unset` returns to the default; `--global` widens a set or unset to every repo. Storage is plain git config under `fufu.<key>`, so `git config fufu.keep` and fufu never disagree, and precedence is git's own — local over global, environment over both.

The earned existence: git config can't say what settings fufu has, what they default to, or whether a value will parse. Every fufu reader falls back to its default on a value it can't read, so a typo'd `fufu.keep` looks set and does nothing. `ff config` closes that gap — a typed registry (size, duration, command, cadence, bool), validation through the same parsers the readers use before anything touches disk, and exit codes that mean something: 0 done, 2 usage or bad value, 1 real failure. Writes are native like everything else: gix's lossless config file, git's own `config.lock` convention, atomic rename, comments preserved. Zero spawns, including the write path.

### `ff update`

jog's self-updater, carried over — and then narrowed to the one thing a self-updater should do. The earned existence: fufu ships six release targets, a tap, and two install scripts — plenty of ways to install, and nothing that keeps an installed binary fresh. What fufu does *not* do is write its own binary: a tool that silently rewrites itself is a compliance finding, and it breaks pinning — mise, nix, or a vendored install places an exact binary and expects it to stay that binary. The auditable shell installer is the only thing that ever writes an `ff`.

So `ff update` is a dispatcher, not a downloader. It classifies how this copy got here and names the command that owns it: `Source` (an unofficial build — dev, dogfood, test) gets `cargo install`; `Homebrew` (a Cellar or brew-prefix path) gets `brew upgrade fufu`; `Script` (the executable is exactly `$HOME/.local/bin/ff`, or `%LOCALAPPDATA%\Programs\ff\ff.exe` on windows — the path the install script writes, and `FF_INSTALL_DIR` deliberately does not widen it, because an exported env var is not evidence of how a binary got here) gets the `curl … | sh` line; and `Unmanaged`, an official build anywhere else, gets the releases page, because whatever placed it replaces it. Only `Script` is acted on, and only after `-y` or a typed yes — everything else prints and exits 0, while `-y` on a channel fufu cannot drive exits 1 rather than pretending. Non-interactive with no `-y` prints the command and exits 0: logging what to run is the honest answer when there is nobody to ask. Running it re-executes the install script over the running binary, which is why both installers land the new file beside the old one and rename rather than writing in place — `rename(2)` replaces the dirent and never touches the busy inode, and windows moves the locked exe to `.old` first and rolls back on failure.

The passive lane notices, and never installs. Official binaries (never dev, dogfood, or test builds; never under CI) spawn a detached `ff update --check` at most once per `fufu.updateCheck` (default daily) — a sanctioned self-spawn in a zero-spawn binary; it refreshes a small cache file under the user cache dir and exits. Foreground commands read that cache and, when a newer release is there, put one line on stderr whose tail names the same channel `ff update` would. *Which* commands read it is a table rather than a habit of wiring — every verb declares the ambient lanes it rides, this one along with the pre-command capture and the daily trim, and the invocation runs them around the dispatch. `ff -v` is one of them, and is the reason the table exists: a flag that printed and exited before any lane could run is how a machine sat a release behind for hours while typing the one command that asks which version it is running. Two throttles keep it polite: the cadence gates the checks, and a release is announced at most once, ever. `fufu.updateCheck false` turns the whole machinery off. The trust root is deliberately plain: the install script's own HTTPS-to-GitHub plus the release's sha256, verified by the script in front of the user rather than inside the binary.

### `ff doctor`

jog's doctor, carried over. The earned existence: a safety net you can't inspect isn't trustworthy — every floor can degrade silently (the log moved by something that isn't fufu, a reflog that never got created, the gc guard deleted from local config, hooks never installed, a stale binary), and without a doctor the first notice is the day the restore you needed isn't there. One command reads the whole net: the engine (the operation log and its age, fufu's identity on the operations carrying it, reflogs, the gc guard, pending foreign drift, settings validated through the same parsers the readers use, the object store's loose-versus-packed split, a trim preview and the auto-trim clock), the wiring (every agent client and shell `ff hook` knows, folded out of the one status vector `ff hook -l` renders — so the two commands cannot disagree — plus a triggers check that warns when nothing at all feeds the capture floor, because a silent engine feels safe while capturing nothing), and the update cache.

Three row levels, jog's shape: `ok` counts nothing, `info` is news not a problem, `WARN` is a finding. Findings drive the exit code — 0 healthy, 1 findings — so scripts and CI can gate on it, and `--json` emits the same rows for machines. Read-only by design: doctor reports the drift the log will absorb and never absorbs it, captures nothing, reconciles nothing. The one consented write is `--fix`, which repairs exactly the two gc reflog-expiry keys — rewriting wrong values where the lazy guard only ever appends missing ones — and nothing else.

## The machine surface

The regimes sort by execution path, and that sorting has been quietly exiling automation: a script, a CI job, an editor plugin, or an agent reaches for git because fufu offers it nothing better to reach for, and takes git's guarantees instead of fufu's. Nothing about a script deserves less than a person at a prompt. So the machine is a first-class reader, and the surface it reads has to be good enough that automation chooses it.

Agents make this urgent rather than merely tidy. An agent edits at machine rate, commits nothing for an hour, and cannot read a status block built to be scanned by eye. Capture already runs before every action it takes — fufu holds a record of that work no other tool has. What is missing is a way to ask for it.

**One model, every surface.** A verb computes one data model, and the human rendering and the JSON rendering are both consumers of it, never translations of each other. `--json` is therefore not a mirror of the human layout: `ff status` crops to two rows because that is what an eye wants, while its JSON carries the model whole. That is what keeps the two from drifting a release apart, and what makes any further surface — an MCP server, a completion source — a thin shell over one contract rather than a second implementation with its own opinions. The JSON is enveloped and versioned (`{"ff": 1, "cmd": "status", …}`) so a script can assert what it is talking to. Notices belong to the model too, not to the margin beside it: anything fufu would tell a person, a script reads as data.

**Every stop names its exits.** Every error carries a stable id, one line of what happened, and the ways out. Prose gets reworded; ids do not, so a script branches on the id instead of matching a sentence, and `ff explain <id>` turns one back into prose on demand. The exits belong to the id, so the registry behind `explain` is where they live and a failure that raises no exits of its own prints those — a raise site overrides only when it knows something the id does not, and an id with nothing to suggest names its own explanation rather than stopping dead. The exits are the accessibility half — fufu is a workflow shift, and where a newcomer bounces is the first stop whose way out they cannot see.

Exit codes carry the same verdict at shell resolution: **0** done, or yes; **1** no — it failed, or the check's answer is negative; **2** the command line was wrong; **3** held — nothing was touched and a human decision is required; **4** contended — nothing was touched and the same command run again is the answer. `3` is the code git has no use for, because only a tool with land-if-clean produces that outcome: `ff sync` exiting 3 is a scriptable "main moved and it needs you," and `4` is its scriptable opposite, a retry with a cap and no decision.

**Every ask has a non-interactive answer.** Wherever this document says fufu asks — the ambiguous parked entry, an undescribed close, an unusual file about to land — a flag supplies the answer up front, and in a non-interactive environment (`FF_NONINTERACTIVE`, or stdin that is not a terminal) the question becomes a structured error naming that flag. No verb ever blocks on a prompt or an editor with nobody there to answer it. `FF_READONLY` completes the pair from the other side: a mode refusing every mutation, so a supervisor or a CI job can be certain a read stayed a read.

**Sessions.** A session is a tag on an operation, and deliberately nothing more. Set one — `--session`, `FF_SESSION` in the environment, the agent trigger stamping its own id without being asked — and every operation recorded while it is set carries it. There is no opening and no closing, so nothing has to end cleanly and a crash costs nothing. There is no verb for listing them either: whoever sets a session knows its name — an agent stamps an id it generated, a person types one they chose — and a tag fufu had to enumerate is a tag fufu had to own.

Tagged operations need not be contiguous. Two agents working at once interleave, and a session is the *set* of operations carrying its tag, never the range between two points. Filtering is the whole purpose (`ff op log 'session(<id>)'`): it buys a question nothing answers today — what did that entire stretch of work change? — and staying a tag is what leaves room to grow later, instead of committing now to boundaries the model would then have to defend. Setting a session is a flag; asking about one is the grammar. `--session` and `FF_SESSION` say what to *tag* — an instruction about what happens next, which no expression can carry — while `session(<name>)` selects the operations already carrying a tag, and rides the same language everything else does. The two never overlap, which is why the flag does not double as a filter: one word doing both jobs would leave `ff op log --session x` ambiguous about whether it was narrowing the log or labelling the capture it takes on the way in. The id rides a trailer on the operation's commit — git objects, legible to anyone who reads them — with a per-repo index as the usual rebuildable cache.

**Two address spaces, and only two.** History has revisions; the log of what fufu did has operations. They never mix in one argument. A revision is a commit sha, a branch name including an anonymous one — a tracking ref is already a branch name in git's spelling — a tag, `@` for the open change, `trunk`, or any of those wearing a git suffix. An operation is a letters-spelled id or `@`, and it appears in exactly two places: as the argument to an `ff op` verb, and after `--at-op`.

**One grammar spans both spaces.** Operations are commits, so the set language reads them too — the same operators, the same functions, over operations instead of over history. What does not carry across is what has nothing to name: no branches, no tags, no `trunk`. `@` is spelled alike in both and means the same thing in each, the newest thing there is.

`@-` is gone from both, and the same rule takes it twice. Naming a commit it is `HEAD` — the open change sits on HEAD's commit, so "the commit under `@`" is what git already says. Naming an operation it is `@^`, because an operation's first parent *is* the operation before it, which makes `@-3` exactly `@~3`, git's own first-parent walk spelled a second time. Aliasing would have been the worse error: git's `@` already means HEAD, so a fufu where `@` is the open change and `@-` is HEAD leaves git's meaning one keystroke from a different one.

Operation ancestry runs along the log, not along the raw parents. An operation's parents carry three unrelated relations at once — the chain in slot one, the base commit in slot two, pins after that — because git has one edge type and nowhere else to put them. The grammar declines to pretend those are one relation: `~`, `::`, and `..` follow the chain, and the base is reached by naming it, `base(<op>)`, which is also the only way an expression crosses from operations back to history. Narrowing to one branch is a predicate, `on_branch(<name>)`, rather than a second ancestry operator; the per-branch link an operation carries is how that predicate is evaluated cheaply, never a spelling of its own.

Op ids are letters on input and never raw hex, even though an operation *is* a commit and its hex would resolve. That keeps hex meaning "commit" everywhere in fufu without exception. The cost is an escape hatch — a sha lifted out of `git log` on the operation ref is not directly usable — and it is worth paying, because the alternative is a hex prefix whose meaning depends on which verb you happened to type it after.

Passing an op where a revision belongs is therefore an error, not a convenience — fufu can tell, since operations carry its own identity — and the error names `--at-op` as the spelling that meant it. Refusing is the point: `ff log` displays each commit's newest op id, so an id seen in a revision-shaped table is exactly the id a user will try to paste into a revision-shaped argument, and a silent success there would teach the wrong model on the first try.

**One grammar, and it is a set language.** Every revision argument goes through one resolver, and every expression in it denotes a *set* — of commits, or of operations where the position takes those. The earned existence is uniformity, which git does not have — `git log <rev>`, `git checkout <rev>`, and a pathspec resolve by three different rules — and it is principle 11 mechanized: a script constructs a target without a mapping table, because there is one grammar to know. Where the expression *sits* follows the same rule one level up: `ff op log <set>` takes it positionally, beside `ff op show <op>` and `ff op diff <a> <b>`, and the position differs only in how many members it accepts. `ff log` keeps `-r` because its own positional slot is reserved for a path — `git log -- <path>`'s question, and operations are not files. One free slot and one spoken for is an asymmetry in the arguments, not in the grammar.

It is `gitrevisions(7)` entire — nothing removed, nothing respelled — plus what git has no way to say: set algebra (`~x`, `x & y`, `x | y`), ancestry as sets (`::x`, `x::`, `x::y`, `..x`, `x..`, `x..y`), and functions. Rule ten decides membership mechanically rather than by taste. `&`, `|`, and `::` earn their place because git cannot express them at all, while jj's `x-` and `x+` are `x^` respelled and its `x ~ y` is exactly `x & ~y`, so all three stay out, and `@-` with them — it names `HEAD` among commits and `@^` among operations, so it is a respelling twice over. Git's suffixes lex as part of the revision token, which is what lets `main~2` keep its meaning beside a prefix `~x` meaning complement: a lexer distinction, not a lookahead rule, so the two never contend.

What is inherited entire is gitrevisions' *revision* grammar — its symbols and its suffixes, handed through unread. Ranges are the set algebra's own, which is why `a...b` is refused rather than inherited: in a set language it is `(a..b) | (b..a)`, so rule ten excludes it exactly as it excludes `x-`. Inheriting a spelling is not the same as inheriting a meaning.

Functions earn their existence one at a time the way verbs do, each against a caller that already exists — `latest` because bare `ff` ranks recent work, `mutable`/`immutable` because the push boundary and every rewrite verb must know what is published, `present` and `coalesce` because a script building a target needs them not to throw, `commit_id` because of the shadowing below. Operations bring their own four for the same reason: `base` because it is the only crossing back to history, `on_branch` because the log spans every branch and one of them is usually the question, and `session` and `kind` because captures outnumber verb operations by more than an order of magnitude and an unfiltered log is mostly machine noise. The set is short on purpose and grows when something calls. `x+` and `descendants` wait on principle 13: children are a reverse walk with no index behind them.

**Ambiguity is refused, with no exception.** This is the one place the grammar cannot be unambiguous by construction — users name branches, and a branch called `dead` is also a valid sha prefix. A token that resolves as both is an error naming both candidates and the spellings that separate them, which is principle 16 with no carve-out. Precedence is the easy answer and the wrong one: silently preferring the branch teaches that a hex-shaped token means a branch here, and on the day someone wanted the commit they get no signal at all.

The escapes need no new syntax. `refs/heads/dead` names the branch and `refs/tags/dead` the tag, both straight out of gitrevisions; `commit_id(dead)` names the commit, and earns its existence because git has no short way to say it. A `branch()` function would be `refs/heads/` respelled, so it does not exist — the same rule that keeps `x-` out. The collision is also narrower than it looks, which is what makes refusing cheap rather than obstructive: a prefix is at least four characters, so only an all-hex branch name of four or more (`dead`, `cafe`, `beef`) can collide at all, and only when a commit carrying that prefix actually exists.

**A set becomes a point by counting.** A verb needing one commit resolves the expression and requires exactly one; zero or many is an error naming what it got — the kind-mismatch promise applied to arity. `ff log -r` is the first consumer of sets, and the reason the language ships now rather than waiting.

Times never enter the grammar at all. Bare ages (`3d`) and date words (`noon`) are legal only where the position's kind *is* a time — `--since`, `--before`, `--at` — so nothing in a revset can be read as a date, `123d` is the commit sha it looks like, and an entire class of shadowing is retired rather than documented. `@{…}` stays exactly what gitrevisions makes it, per-ref history, and belongs to the revision space alone; the operation log answers the same question better and across every ref at once, which is what `--at` is for.

The same discipline governs arguments generally: **a positional argument has exactly one kind.** Where a verb needs a second kind it takes a flag, which is why `ff restore <path> --from <rev>` puts paths in the position and the source behind a flag. fufu never inherits git's `--` disease, where one position means a rev or a path and a separator arbitrates.

That is also what keeps `ff restore` and `ff op restore` from being two spellings of one idea — jj's arrangement exactly, and it works there for the same reason: the two share no argument. `ff restore` takes paths, and moves file content. `ff op restore` takes an operation, and moves the whole repository. The `op` prefix is not decoration; it announces which address space you are in, so there is no way to half-write one and land in the other. `ff undo` is the everyday shortcut for the second, argument-free and repeatable, and most users will never type either long form.

Verbs still mean a *kind*, and a kind mismatch redirects rather than refuses. `ff switch <sha>` has exactly one sensible reading, so fufu mints an anonymous branch there and says so, naming `ff describe -b <name>` to name it and `ff start` as the verb that meant it; `ff edit <branch>` already redirects the other way. Acting is not guessing: one available reading is taken and announced, while more than one is an error naming the candidates — which is why trunk resolution still refuses to pick.

**Extension.** Three mechanisms, all of them git idioms:

- `ff <name>` runs `ff-<name>` from PATH when no built-in verb matches — git's own extension model, with the repository, the machine contract, and the current session tag handed down in the environment.
- `ff watch` streams operations as they land, newline-delimited JSON. The operation log is already an event log; this gives it subscribers — status lines, editor plugins, dashboards, agent supervisors. `--all` widens it from this worktree to every chain in the repository, one line per motion with the worktree named on it, which is what turns a pool of bays from N processes and N sets of anchors into one stream a supervisor can key on. The chain set only grows: a bay that appears mid-stream is announced and joined, and a bay that is retired keeps its place, because the removal captures into its chain before the directory goes. A rewrite is terminal per chain rather than per stream — one bay's trim must not end another bay's tail. It is not a daemon: it is a foreground process the user started, and it holds no authority.
- `ff-core` is a library before it is a binary. Publishing it is the deepest hook and the largest promise, so it waits until what it would freeze has stopped moving.

The rule that keeps extension from eating the invariant: **extensions read fufu state and call fufu verbs; only fufu writes fufu state.** A plugin editing `refs/fufu/*` is a second author of a cache whose entire safety argument is that it has one — principle 3, with the author named.

Hooks are the oldest mechanism and the widest, and they are two verbs rather than one because they are two contracts. `ff hook` and `ff unhook` are what a person types: flat, permanent slugs — `claude`, `codex`, `cursor`, `gemini`, `bash`, `zsh`, `fish`, `powershell` — where an unknown name is a real error and a failure is loud. `ff trigger <source>` is what a client calls, and its contract *is* the extension point: always exit 0, never veto, fail silently, `FF_DEBUG=1` to see why, and an unrecognized source name exits 0 in silence rather than erroring. That last clause is what makes a fufu trigger safe to wire into a tool fufu has never heard of, so it is published rather than reimplemented per vendor.

The two namespaces are deliberately different, because `hook` names a thing you integrate with and `trigger` names an event source, which is finer-grained. Every shell slug installs rc lines calling one `ff trigger shell`. The shell wires two independent pieces, and both of them capture: the `git` alias snapshots before a git command, and the prompt hook snapshots at every prompt. Neither says anything — where the alias is silent because it is standing in front of git, the prompt hook is silent because a line above the prompt is noise and the snapshot is the whole point. Leaning on Enter is free: an unmoved tree captures to `NoOp`. A client whose payload identifies its own event is one source name; one whose payload cannot is `<vendor>-<event>`, resolved by splitting on the first `-` and forcing the event from the tail. `manual` is a source with no slug, because there is nothing to install.

Behind both verbs sits one client-neutral core — a neutral event, one shared capture pipeline, one briefing text — and four thin protocol adapters that translate a payload in and wrap the briefing on the way out. Two things stay vendor-visible on purpose. The snapshot's subject keeps the source's own name (`claude[a1b2c3d4]: Edit(src/x.rs)`), because a subject says who. And the briefing marker is per-slug, because two clients in one repository sharing one marker would each clobber the other's session id and re-brief forever. It records an audience as well as a session — the empty name is the main thread, the rest are agent ids — because a subagent inherits the parent's session id, fires no prompt event, and was told nothing: it is a context of its own, so it is an entry of its own. A payload also carries more events than the briefing rides. Turn end is a capture lane and nothing else: capture is snapshot-*before*, so the file state an agent writes as its final action would sit uncaptured until whatever came next, and a session that ended there would never snapshot it at all.

What an agent is *told* is two texts, budgeted differently on purpose. The briefing is the always-on contract, and it is present wherever a context is built or a new audience starts work: a boundary re-briefs unconditionally — a resume, a `/clear`, a fork, a compaction all hand back the same session id and drop or truncate the context the briefing was injected into — and every other audience is briefed exactly once, on the first tool call fufu sees from it. The cost is a multiple of one copy rather than one copy, which is the trade: it is capped by test at what four writing verbs, the git rule, and one pointer to `--help` actually cost, and a session with several subagents and a compaction pays that several times. Everything fufu has to say on one event goes out as one reply, because a client parses a hook's stdout as a single object and the briefing and a `fufu.gitPolicy` correction can both fall due on one tool call. The marker is stamped only when that reply actually printed — three of the four adapters have no channel on a tool — because a marker stamped against a reply that went nowhere would lose that repository's briefing permanently. Everything past that — recovery, rewriting commits that have closed, held rewrites, the machine surface — is a skill fufu ships and installs beside the wiring, and a skill costs nothing until a client decides the situation calls for it. That split is the whole reason both can be good: the briefing stays short because it is not carrying the manual, and the manual stays complete because it is not being charged per session. Both are prose an agent reads as instructions, so both rot the way prose does and neither is allowed to: one test parses every command either text spells and fails on surface clap no longer takes.

Delivery is the same directory story the plugin already tells. Claude Code's skill rides inside the plugin fufu owns outright; Codex takes a directory of its own beside the settings file it does not. Cursor and Gemini read no skills directory, so they get the briefing alone and are not told to read something that is not there — which is why the question is asked of the adapter at print time rather than assumed from the install. The skill is also printable: `ff hook --skill` writes the same bytes an install would to stdout, which is what makes the manual reachable on a client that reads no skills directory and on one fufu has never heard of — the half of the published extension point that was missing, since `ff trigger <unknown>` already invites a third party to wire capture. The block at the end of `ff --help` is what routes an agent to it, and carries the same guard the briefing does: a test parses the command it names.

A hook never vetoes on its own judgment; the wish to stop an agent from touching a branch is policy, and policy lives in config — so the one veto there is, `fufu.gitPolicy strict`, is config saying so, and it travels as JSON the client is free to ignore rather than as an exit code. `ff trigger` still always exits 0.

## Substrate

Rust, on gitoxide (`gix`). The rule that governs execution: **git defines the
semantics; fufu chooses the execution per call-site.**

- **Reads are native from day one.** A capture runs at every shell prompt, and
  subprocess spawn cost (5–15ms per `git` exec) can't carry it. Refs,
  objects, index and status reads, log walks: in-process. Floor 2's core
  primitive too — merge simulation runs in memory (gitoxide's merge-ort port),
  cached by (base, ours, theirs), so futures recompute only when a ref moves.
- **Writes climb a ladder as trust grows.** Object writes (capture snapshots,
  stash entries) go native early — jog-proven territory. Disk-materializing
  operations (checkout through filters, rebase, push with auth) start on the
  git binary and go native as coverage earns it.
- **The wire is climbed, except for sending.** `ff clone` and `ff sync`'s fetch speak the git protocol themselves — gix's blocking transport over reqwest and rustls — so the negotiation, the pack and clone's checkout happen in-process, and no porcelain is spawned to do it. What they still reach outside the process for is git's *configuration and authentication* surface rather than its porcelain: one `git config -l` per process, so `url.<base>.insteadOf`, `http.proxy` and `credential.helper` from the installation config are honored rather than ignored; a credential helper when a remote asks for auth; `ssh` for an ssh URL; and `git-upload-pack` for a filesystem remote, because a local transport *is* a spawned upload-pack, in git no less than here. Native is therefore a claim about the protocol and the porcelain, not about the process table: over http(s) the whole conversation is fufu's. One repository shape is handed back to the porcelain: a linked worktree's admin dir holding a `gitdir` file without a readable `commondir` — the state one passes through while it is being created or removed — is a directory git's worktree walk skips and gix's fetch fails the whole fetch on, so `ff sync` retries that fetch once through `git fetch` and reports the broken admin dir if that fails too. `ff publish`'s push is the one that stays spawned, and not for want of trust — gix speaks the half of the protocol that receives a pack and nothing that sends one, so there is no rung there to climb yet. `ff init` reaches nothing at all.
- **Differential testing is the compatibility contract.** Every native
  operation is checked against git-binary output in CI, permanently. A native
  merge that differs from git's by one edge case silently breaks the invariant;
  compatibility is a standing test suite, not a port milestone.
- **Behavioral compatibility included.** Correct formats aren't enough: if
  `ff commit` writes commit objects directly, the user's pre-commit and
  commit-msg hooks still run — fufu execs them itself. That extends to what
  the hooks *see*: a hook-runner like lefthook, lint-staged or husky asks git
  what is staged and does nothing when the answer is empty, so before the
  first hook fufu writes the index to the tree it is about to commit — the
  slice for a partial `ff commit <paths>`, matching git's pathspec form — and
  restores it byte-for-byte when the close does not land, as git rolls its own
  index back after a refused `commit -a`. The index stays a derived surface
  the user never sees or maintains; it was simply being written at the wrong
  moment. One divergence stands, in fufu's favor: a formatter's fixes land via
  the worktree re-scan rather than via anything the hook staged, so lefthook's
  `stage_fixed: true` is decorative here, where under git a formatter that
  rewrites without re-staging loses its fixes. Boring citizenship covers
  behavior, not just bytes.
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
    accumulated — how deep the history, how long the operation log, how many
    entries it holds. Capture runs before every agent action, so a
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

The machine surface is not a phase but a constraint on every one: one model with two renderers is a Phase 0 substrate decision, error ids and exit codes arrive with the verbs that raise them, sessions ride the capture floor in Phase 1, `ff watch` follows the operation log in Phase 2, and the generated surfaces — MCP, completions — land with adoption in Phase 5.

**Phase 0 — Bedrock.** Rust workspace on gix; the native read core (refs,
objects, index, status, log); the differential test harness against the git
binary, which lives forever after; read-only `ff status` and `ff log`. Proves
the substrate and the zero-spawn budget before anything depends on them.

**Phase 1 — Capture.** Floor 1 rebuilt native: the snapshot engine, with every
ff command capturing first; the per-branch timeline
interleaved into `ff log`; `ff restore`; manual retention (`ff trim`). (Phase 1
shipped a manual snapshot verb on bare `ff`; it retired when bare `ff` became
the map, and came back as `ff trigger` — capture is automatic *by default*,
with one verb that forces it and `-m` to say why.) The
`ff git` passthrough and the recommended alias move up from Phase 5: anything
reaching for git grabs fufu instead from day one. Triggers are the
capture-first commands, the alias, and the agent clients (Claude Code, Codex,
Cursor, Gemini CLI); editor integration is deferred until a real need shows up. jog's lessons carried
over, its code not owed. From here on, nothing can be lost.

**Phase 2 — Time.** The operation log and whole-repo `ff undo`; reconciliation as
a first-class deliverable (cache-not-authority needs machinery, not vibes);
tree memory via driven stash — `ff switch`, `ff start`, `ff branch`, anonymous
branches and the naming rename — and `ff commit` closing the open change (fufu's
first index write: a close must leave `.git/index` matching the new HEAD, or
foreign `git status` shows phantom changes). The daily driver exists after this phase.

*Phase 2 implementation notes (shipped Aug 2026).* The operation log is one
commit chain per worktree at `refs/fufu/wt/<id>/ops`, captures and verbs
together: parent 1 the
previous operation — reserved for it, never a pin, so a first-parent walk *is*
the log — parent 2 the base commit, parent 3 the record for the operations
that change refs, pins after that. Reachability is the gc pin, and appends
serialize on a lock fufu takes itself: gix compares a ref's expected value
against one it read *before* locking, so the CAS catches a stale plan and not
a second writer. A capture changes no ref by invariant, so
it carries no record and costs the one commit a snapshot always cost, which is
what let the two chains merge without doubling the store. Per-branch access is
a pointer at `refs/fufu/snap/<branch>` plus a back-link on every operation, so
one branch's history is a walk and not a filter. Every mutating verb records
write-ahead (planned post-state before mutating); a crash between append and
mutation is labeled "may not have completed" by the next reconcile. Foreign
motion is absorbed as one `foreign` operation per pass, quoted with git's own
reflog messages, undoable and labeled; the notice stays pinned in `ff status`
while the log tip is foreign. Parks are byte-shaped `git stash push -u -m
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
until Phase 4 owns merges. Retention rides the same `fufu.keep`
knob through `ff trim` — trash-first at the chain's own
`refs/fufu/wt/<id>/trash/@ops`, pin
parents preserved verbatim, prev links rewritten — and the oldest survivor
becomes the undo floor. Petnames are `ff/<adjective>-<noun>` from embedded
wordlists. Two-phase descriptions live in plain JSON files under
`<common-dir>/fufu/branch/<branch>` (pending description, fork base, parent branch),
recorded on every change so `ff undo` restores the text.

**Phase 3 — Futures.** In-memory merge simulation, cached; `ff status` starts
reporting futures, not just facts. Pure reads — no automation yet, just
foreknowledge. An ambient shell channel — a status line at pause points — shipped
in this phase and was withdrawn: a conflict verdict appearing above the prompt
unbidden reads as an error, and the prompt hook's rc lines now take a snapshot
and print nothing. The verdicts it rendered are still one `ff status` away, asked
for rather than volunteered.

*Phase 3 implementation notes (shipped Aug 2026).* The verdict is a
commit-by-commit replay, not a single endpoint probe: it costs N in-memory
merges instead of one, but it matches what `ff sync` will really do and it can
name the commit that breaks. The whole replay runs inside one
`with_object_memory` clone — intermediate trees are written there and read back
by the next step — so a probe writes nothing, which is asserted by counting
loose objects around one. One merge per commit with plain options, the conflict
list deciding; `stash.rs` probes and then re-merges only because it needs the
tree to persist, and futures never do. A branch answers to two things and they
are measured on separate axes: the base beneath it and the remote copy of
itself. Bases come from a ladder: an explicitly recorded parent branch, else
trunk — and nothing after that, because when trunk *is* the branch underfoot
there is no base, only a remote. Reaching for the upstream there was what made
`ff status` report one fact twice in two dialects.
**Trunk ambiguity is swallowed to "no base", never propagated:**
a repository that cannot name its trunk still gets a working `ff status`. For
the same reason `BranchMeta.parent` records only an explicit fork target; a bare
`ff start` records none, so trunk stays live rather than frozen at mint time.
Up-to-date is tested before fast-forward, because equal tips satisfy both and
announcing a fast-forward of nothing is a lie. A branch that has already merged
its base reads up-to-date rather than unknown — nothing of the base's is left to
integrate, and while a real rebase would still linearize it, that is the branch
rewriting itself and not a cost the base imposes. Merge commits inside the range
and depths past `fufu.futuresDepth` are honest `Unknown`s, because a wrong
verdict is worse than an admitted silence. The cache under
`<common-dir>/fufu/futures/<branch>` holds a slot per axis, each keyed by its
own four inputs (the ref measured against, its tip, the branch tip, the open
tree), so it is self-invalidating: no eviction policy, no staleness clock, and
deleting it changes no answer, only the cost of getting one. A remote
configured against a ref that is not there short-circuits to `gone` without
probing or caching, there being nothing to simulate and nothing worth
remembering.

**Phase 4 — Rewrite.** Floor 3: the rewrite map, `ff absorb`, `ff describe`,
`ff edit` sessions with `ff done`, held rewrites and `ff resolve`, and `ff sync` —
both axes land-if-clean, plus the outgoing half it cannot honestly ship without:
lease semantics and the held-rewrite guard. The jj-grade workflow lands here, safe
because phases 1–3 are underneath it.

**Phase 5 — Adoption.** The name and packaging sweep. (The `ff git` passthrough and
alias shipped with Phase 1; by here `fufu.gitPolicy` has grown a correction
for every verb worth naming, and `ff sync`'s outgoing
half came forward into Phase 4 — a sync that rebases onto a moved trunk and
cannot then publish leaves the branch diverged behind a plain `git push` that
fails, which is the footgun the verb exists to delete.)
The tool becomes recommendable to someone who isn't its author.

**Phase 6 — Git-free.** The long tail moves native — push, checkout through
filters, hooks exec'd by fufu — until the git binary is an optional neighbor
rather than a dependency. Fetch landed early, with clone; push waits on gix
growing a send-pack, which is the one item here fufu cannot finish alone. Done
when a machine with only `ff` on it is a working development machine.

## Open questions

- **Snapshot mechanics at fufu's write rate** — jog's capture cadence is
  prompt/command-shaped; land-if-clean automation may want cheaper, more frequent
  capture. In-process object access changes the calculus jog was built under;
  revisit ref layout and caching with gix in hand.
- **Tree memory residuals** — parked entries orphaned by foreign branch
  deletion; whether to set `status.showStash` so plain `git status` mentions
  parked work. The same-branch-in-two-worktrees half of this is answered:
  fufu enforces git's exclusivity itself now, so the state cannot be reached.
- **Anonymous-branch hygiene** — unnamed branches accumulate; `ff branch list`
  segregates them (Phase 2) and the name scheme and metadata
  home are settled (`ff/<adjective>-<noun>`; JSON under
  `<common-dir>/fufu/branch/`), but a tidy of merged or abandoned anonymous
  branches remains open.
- **Rewrite-map hygiene** — divergence is settled by cache-not-authority (an entry rewritten outside fufu is invalidated, loudly), and the map's home is the operation record, which makes pruning `ff trim`'s job. What is left is the lookup index, still deferred: the first reader outside the tests — sync asking whether a divergence is its own — answers a handful of shas once per invocation against a walk the queried commits' own timestamps bound, and did not need one.
- **Reconciliation triggers** — lazy (rebuild at the next fufu invocation,
  jog-style) versus live (post-commit / reference-transaction hooks). jog's
  no-git-hooks stance costs freshness, and nothing reads fufu state on its own
  any more to make that cost visible.
- **Held-rewrite composition** — one hold per branch is settled: a second
  *conflicting* rewrite refuses rather than guessing an order, while one that
  would land cleanly is not competing for anything and goes through. What stays
  open is ordering across a stack of branches, and whether a hold should ever
  expire on its own rather than at the next question.
- **Resolution absorb-back edges** — settled: an edit outside every marked
  region belongs to the last step, because the marker tree *is* the
  post-rewrite tip's tree, so nothing lands in a commit the reader never looked
  at. What stays open is the second round — a tangle is held again rather than
  materialized, so the reader fixes what they were shown and is told there is
  more, without being shown it until they ask.
- **Edit-session boundaries** — fufu's own verbs are settled (close verbs
  attempt `done` land-if-clean; `ff switch` parks the session; explicit
  abandon), but: what a *foreign* switch or commit does to an open session;
  how capture attributes mid-session snapshots (to the target commit, not the
  tip); whether editing a published commit warns or refuses; and how a
  session-turned-held-rewrite composes with the parked tip state.
- **Undo across foreign events** — settled in Phase 2: a reconciled foreign
  mutation is an operation, `ff undo` rolls it back like any other, the
  report labels it as a change made outside fufu. One rough edge stays open:
  an operation that recorded no index — a foreign one, because fufu wasn't
  running, or a capture, because it carries no record — undoes to a clean
  index at that operation's own HEAD tree.
- **Auto-sync policy surface** — the verbs and the divergence rule shipped, and
  the split into `ff sync` and `ff publish` settled the half of this that was
  about kind: a fetch that fast-forwards can only fail to be useful, while a push
  leaves the machine, and they are no longer one act to decide about. What stays
  open is whether an upstream-tracking branch ever syncs *without* being asked,
  and on what trigger. Only the local half is a candidate.
- **Agent adoption** — the machine surface makes fufu usable by an agent; what
  makes an agent reach for `ff` instead of `git` was the open half, and
  `fufu.gitPolicy` is the answer: one graduated setting over both entry
  points, correcting only the git words fufu has a verb for and never
  rewriting a command line. The other candidate shipped as `ff mcp`: one
  tool over stdio whose every call is an ordinary `ff <verb> --json` child,
  registered by `ff hook <client>`, so the path of least resistance and the
  shell command are the same contract.
- **Sessions past a tag** — one tag per operation answers filtering and nothing
  else. Left open until something needs it: whether an operation may carry more
  than one (an agent's inside a person's), and what a tag means once a rewrite
  folds its operations into a commit. Also the word itself, which already names
  `ff edit`'s editing session — two meanings, one term.
- **Name/packaging sweep** — `fufu` and binary `ff` against crates.io, npm,
  Homebrew, apt before anything ships. (Known neighbors: `ffuf` the web fuzzer,
  `fuf` an obscure file browser — distinct, but check registries.)
- **Substrate maturity tracking** — which daily verbs must stay binary-backed at
  v1; gitoxide's push support and filter pipeline are the pacing items for
  git-free. (Language/runtime itself is settled: Rust on gitoxide — see
  Substrate.)
