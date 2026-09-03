# The push boundary

**`ff sync` takes in; `ff publish` sends. Two verbs, because everything sync does is undoable and a push is not.**

Every operation fufu performs on your machine lands on the [operation log](snapshots-and-undo.md), where one `ff undo` takes it back. A push is the one act that leaves the machine: the moment it lands, other clones can fetch it, CI runs on it, webhooks fire, and no operation log on your machine reaches any of that. So fufu splits reconciling with a remote along exactly that line. The incoming half is a routine verb with the full undo guarantee; the outgoing half is a verb you type on purpose, and it never rides along as a default inside anything else.

## Sync is the incoming half

[`ff sync`](../reference/cli/sync.md) brings every local branch up to date with the two things it answers to: the base it sits on and the shared copy of itself, and [tracking](branches.md#tracking-one-branch-one-shared-copy) means there is exactly one shared copy to answer to. It fetches once, then replays each branch's commits onto whatever moved, with the branches stacked above following.

For the shared copy, sync asks two questions of each branch. Have you changed this branch since you last saw its shared copy? If not, it follows the shared copy wherever it went. If you have, is what the shared copy holds beyond you new work, or old versions of yours? New work is taken in and your commits replay on top. Old versions of yours are left alone for publish to replace; fufu knows them because it recorded the rewrite, or the publish you undid. Plain git cannot tell those two apart, which is the whole reason fufu keeps the record.

The replay runs in memory and lands only when it is clean. A commit that conflicts holds the branch it belongs to: nothing moves there, no half-applied tree touches the repository, the run goes on to the next branch, and [`ff resolve`](../reference/cli/resolve.md) on that branch picks the [held rewrite](held-rewrites.md) up at a moment you choose.

Nothing sync does leaves the machine. The fetch, the replay, the whole run is one operation, and one `ff undo` takes it back. That guarantee is the whole reason sync stops where it does: when your branch is ahead of its shared copy, sync names what is waiting and leaves it.

## Publish carries a lease

[`ff publish`](../reference/cli/publish.md) sends the branch to its remote. The push carries a lease: it goes through only if the shared copy still stands where you last saw it. If somebody pushed since, nothing is sent and nothing is lost — `ff sync` takes their work in first, and publish sends afterward.

Publish does not fetch first, on purpose. The lease is worth something precisely because it means the tip you last read; fetching just before pushing would refresh the lease to a tip you never looked at, and git would then be guarding you against a change you accepted sight unseen. Keeping fetch out of publish is what keeps the lease honest.

A [held rewrite](held-rewrites.md) blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under it.

## Rollback is undo, then publish

There is a way back from a push, and it is this same verb rather than `ff undo` alone. [`ff undo`](../reference/cli/undo.md) moves your local branch back — [a pointer move on the operation log](snapshots-and-undo.md), like any other undo — and the next `ff publish` finds the shared copy standing ahead of where you now do, at a tip fufu itself sent. fufu records every push, so it knows which commits out there are your own, and moving the shared copy back over your own push is a different act from moving it back over a teammate's. The rollback goes out under a lease like any other publish: if somebody pushed onto the branch since, it stops instead of taking their work with it.

Rollback is not erasure. Other clones may already hold the commits, CI already ran on them, a webhook already fired. What rollback promises is narrower: the shared copy stands where your branch does, and anyone who syncs from now on takes in the rolled-back line. Commits that reached the world stay reached; the branch simply stops pointing at them.

## Four pushes, one verb

Publish is one verb wearing four different acts. A branch with no shared copy yet gets one created, with tracking set up in the same step — and with several remotes, [`--to <remote>`](../reference/cli/publish.md) names where it answers, once, because [one branch has one shared copy](branches.md#tracking-one-branch-one-shared-copy). A branch whose shared copy stands behind it gets that copy replaced under the lease. A branch whose shared copy was deleted gets it put back, under a lease that says it must not exist — and telling a deleted copy apart from one that never existed is another reason fufu keeps a record of what it has sent. And a branch you undid gets its shared copy rolled back.

`ff publish --dry-run` says which of the four this push would be, without making it. It writes nothing and sends nothing, and it is the way to ask while the answer still costs nothing.

## Published history is append-only

fufu's opinions about history — commits malleable until they are shared, branches rebased onto their base, force-pushes to your own leased branches as routine — stop at this boundary. They apply to work only you hold: unpublished commits, and the shared copies of your own branches, which the lease and the push record make safe to move. History the team shares is append-only, and fufu has no verb that rewrites it.

How work lands on the shared branch — merge commit, squash, rebase — stays the team's and the forge's business, not fufu's. Inside your own work fufu is opinionated; in everything the rest of the world can see, it is indistinguishable from careful use of plain git. That is [the invariant](invariant.md) holding at the one place work leaves the machine.
