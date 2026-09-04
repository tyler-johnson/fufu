# fufu vs git

**fufu replaces git's daily rituals, and nothing underneath them.** The repository stays an ordinary git repository at every instant — [the invariant](../concepts/invariant.md) — so adopting fufu changes what you type, never what your teammates, your forge, or your tools see. This page covers the trade from your chair: the commands that disappear, what stands in for each, the opinions you are accepting, what stays git's, and what the trade honestly costs.

## What disappears

The commands fufu retires are the dangerous-but-daily ones — the rituals that exist to move state by hand that git could have moved for you.

**`add` and the staging area.** There is no index to maintain between commits. [The working tree is the change](../concepts/changes.md): [`ff commit`](../reference/cli/commit.md) closes it into a commit in one step, and a partial commit is a slice picked at the moment of the close — path arguments naming files or directories — with nothing persisting afterward.

**The stash dance.** [`ff switch`](../reference/cli/switch.md) parks whatever is open with the branch you leave and reopens whatever was parked where you arrive, untracked files included. The stash itself survives — a parked change is an ordinary stash entry labeled with its branch, visible in every GUI's stash panel — but the two-step dance, and remembering which entry belonged to which branch, is gone.

**Detached HEAD.** Editing a commit mid-stack is [`ff edit <rev>`](../reference/cli/edit.md), which mints an anonymous branch at that commit and switches to it; [`ff done`](../reference/cli/done.md) amends, replays the commits that were ahead, and returns you to the tip. HEAD stays attached to a branch throughout, because a detached HEAD is a state fufu never creates.

**`rebase -i`.** Its jobs split into verbs that each do one thing: [`ff describe <rev>`](../reference/cli/describe.md) rewords any commit, [`ff absorb`](../reference/cli/absorb.md) folds working changes into a past commit — which also retires the `fixup!`-plus-`--autosquash` ritual — and `ff edit` covers the `edit` stop. In every case descendants restack automatically and the replay runs in memory, landing only when clean; a conflicting replay becomes a [held rewrite](../concepts/held-rewrites.md) you resolve on your schedule, so the stop-fix-continue treadmill goes too.

**The rebase itself.** [`ff sync`](../reference/cli/sync.md) lines the branch up with its base and its remote in one verb, fetch included, and [`ff restack`](../reference/cli/restack.md) is the offline replay for a branch you name. Both are recorded operations that one `ff undo` takes back whole.

**The reflog as a recovery tool.** Every operation — every automatic snapshot included — lands on [one operation log](../concepts/snapshots-and-undo.md) that records refs and tree together. [`ff undo`](../reference/cli/undo.md) steps the whole repository back, and [`ff history`](../reference/cli/history.md) shows exactly where each press would land. The reflog still exists and still fills; you stop needing to read it.

The reflex-by-reflex mapping — what you would have typed in git, and what to type now — is the [command table](command-table.md).

## What your aliases cannot do

A git veteran's first response to the list above is that aliases and scripts already cover it. For the typing, they can: an alias can spell `commit -am`, and a script can stash, rebase, and pop.

### It cannot act before you type

fufu snapshots the working tree before every mutating command — before a switch parks your tree, before a sync replays it, before `ff git` hands your arguments to git — so the state a mistake would destroy is already saved by the time the mistake is possible.

A safety net you have to remember to throw is a checkpoint, and the manual checkpoint is exactly the ritual [snapshots and undo](../concepts/snapshots-and-undo.md) exists to delete.

### It cannot give one account of what happened

Wrappers leave their records where each underlying command left them — some motion in the reflog, some files in the stash, some state nowhere at all — and reconstructing an afternoon means reading all three.

Every fufu operation, captures and raw-git motion included, lands on [one log](../concepts/snapshots-and-undo.md). So `ff history` is the whole account, and one `ff undo` steps the repository back through it, refs and tree together.

## The opinions, and where they stop

Adopting fufu is partly a workflow shift, not a transparent overlay. Its verbs encode positions, and using them is accepting the positions:

- **Branches rebase onto main rather than merging it in.** `ff sync` replays your commits onto the moved base; there is no verb that merges trunk into a feature branch.
- **Unpublished commits are malleable by default.** Rewording, absorbing into, and reshaping commits that only you hold is routine, and every rewriting verb keeps descendants following automatically.
- **Leased force-pushes to your own branches are routine.** [`ff publish`](../reference/cli/publish.md) always pushes under a lease, so replacing the shared copy of your own branch after a restack is the normal case — guarded, not exceptional.

The invariant promises compatibility, never neutrality: the repository stays legible to every tool and teammate, and fufu still has opinions about how you work inside it.

All three opinions stop at [the push boundary](../concepts/push-boundary.md). Published history is append-only — fufu has no verb that rewrites history the team shares — and how work lands on the shared branch (merge commit, squash, rebase) remains the team's and the forge's business. Inside your own unpublished work fufu is opinionated; in everything the rest of the world can see, it is indistinguishable from careful use of plain git.

## What stays git

**The repository format.** Objects, refs, config, worktrees — everything on disk is git's own, written the way git writes it. fufu's records — the operation log, snapshot refs, parked entries — live in ordinary refs, and they are a cache over the repository, never an authority over it.

**Remotes and forges.** A remote is a git remote, a push is a git push, and GitHub or any other forge sees ordinary branches and ordinary force-with-lease updates. Nothing server-side knows fufu exists.

**Hooks.** Your `pre-commit` and `commit-msg` hooks still run — fufu execs them itself — and hook-runners like lefthook and husky see a staged index matching what is about to land, because fufu writes the index before the first hook fires.

**How work lands.** The merge queue, the squash button, rebase-and-merge — the landing policy is untouched, per the push boundary above.

**Everything else.** Bisect, submodules, plumbing, forensics stay git's, reached through [`ff git <args>`](../reference/cli/git.md), which snapshots first and then runs git verbatim.

## The honest costs

**Undo is a new mental model.** Trained git reflexes reach for `reset --hard`, `checkout -- <file>`, `stash pop`, and reflog spelunking — each a different tool with a different blast radius. fufu replaces them all with one log and a few verbs, which is simpler once it is reflex and disorienting until then. For the first stretch you will know git's incantation and have to look up fufu's.

**Verbs do not map one-to-one.** `rebase -i` alone splits across several verbs, and `checkout`'s jobs scatter to `switch`, `restore`, and `edit`. The [command table](command-table.md) shortens the search, but most of its rows carry a footnote because most mappings are inexact, and retraining muscle memory takes real days.

**You are trusting automatic capture.** There is no checkpoint verb; the safety net is the snapshots fufu takes on its own, which means the net is only as strong as its wiring. Capture fires on fufu commands, at shell prompts, and around agent tool calls only where the [hooks](../reference/cli/hook.md) are installed, and work done around fufu is protected only as far back as the last capture — a raw `git restore <file>` can discard edits fufu never saw. [`ff doctor`](../reference/cli/doctor.md) reports the state of the net, and checking it occasionally is part of the deal.

**The opinions are not optional.** If your team's habit is merging main into feature branches, or your workflow leans on a hand-maintained staging area, fufu will not meet you halfway — the verbs for those workflows do not exist. `ff git` keeps every git command reachable, but working against the grain of the opinions costs more than either tool alone would.
