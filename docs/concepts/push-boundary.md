# The push boundary

**`ff pull` updates local branches; `ff push` updates remote branches. Undo can restore recorded local changes, but cannot reach a remote push.**

Local branch and file changes are recorded on the current worktree's [operation log](snapshots-and-undo.md). Fetches and ambient maintenance have their own effects outside that undo step.

A push updates the remote branch. Other clones can fetch it, CI can run, and webhooks can fire — no operation log on your machine reaches those effects.

The outgoing half is a verb you type on purpose, and it never rides along as a default inside anything else; it has no `--all`, so every branch that leaves the machine is one you named or one you stand on.

## Pull is the incoming half

[`ff pull`](../reference/cli/pull.md) brings a branch up to date with the two things it answers to: the base it sits on, and the shared copy of itself on the remote. Bare, that is the branch you stand on; names take others, and `--all` every local branch. [Tracking](branches.md#tracking-one-branch-one-shared-copy) means there is exactly one shared copy to answer to.

It fetches once, then replays each branch's commits onto whatever moved, with the branches stacked above following.

For the shared copy, pull asks two questions of each branch:

- **Have you changed this branch since you last saw its shared copy?** If not, the branch simply follows the shared copy wherever it went.
- **If you have, what does the shared copy hold beyond you?** New work is taken in, and your commits replay on top. Old versions of your own commits are left alone for [`ff push`](../reference/cli/push.md) to replace.

fufu can tell those two apart because it recorded the rewrite, or the push you undid. Plain git cannot, which is the whole reason fufu keeps the record.

The replay runs in memory and lands only when it is clean. A commit that conflicts holds the branch it belongs to: nothing moves there, no half-applied tree touches the repository, and the run goes on to the next branch. [`ff resolve`](../reference/cli/resolve.md) picks that [held rewrite](held-rewrites.md) up at a moment you choose.

Pull sends no branch updates to the remote. Its local branch and working-copy changes form one undoable operation; fetched objects and tracking-ref updates are separate. `ff pull --dry-run` still fetches, writes objects and remote-tracking refs, prunes deleted tracking refs, and fetches tags. It previews the replay without changing local branches or files. `--no-fetch` skips that fetch. Successful invocations can still run automatic trimming and update maintenance, including under `--dry-run --no-fetch`.

## Push carries a lease

`ff push` sends the branch you stand on to its remote, or the branches you name, from wherever you stand. Each push carries a **lease**: the remote ref must match the expected tip when Git updates it. Replacing commits requires that fufu's recorded seen tip agree with the tracking ref. A fast-forward can proceed without that agreement because it removes no commits; creating or recreating a copy requires the remote ref to be absent. Among several, a refused lease is that branch's alone; the rest still go out, and the report says which.

If the lease or seen-record check refuses a branch, that branch is not sent. `ff pull` takes in the remote work before you retry.

Push can auto-fetch on `fufu.autoFetch`'s cadence before planning; `--no-fetch` skips it. That fetch updates tracking refs without refreshing fufu's seen record. The record is written by `ff pull`, [`ff switch`](../reference/cli/switch.md) onto a remote's branch, [`ff clone`](../reference/cli/clone.md), and a successful push. A fetch by an editor, `ff git fetch`, or `ff pull --dry-run` likewise does not authorize replacing an unseen tip. A non-fast-forward push after such a move is refused before the wire; the wire lease also catches later races.

A [held rewrite](held-rewrites.md) blocks the exit. Nothing is sent while the branch's commits are still about to be rewritten out from under it.

## Rollback is undo, then push

There is a way back from a push, and it is this same verb rather than `ff undo` alone.

[`ff undo`](../reference/cli/undo.md) moves your local branch back, a pointer move on the operation log like any other undo. The next `ff push` then finds the shared copy standing ahead of where you now do, at a tip fufu itself sent.

fufu records pushed tips and the tips it has seen. Those records support rollback and reconciliation; they do not identify a branch's owner or grant permission to rewrite it.

The rollback goes out under a lease like any other push. If somebody pushed onto the branch since, it stops rather than taking their work with it.

### What rollback promises

Rollback is not erasure. Other clones may already hold the commits, CI already ran on them, a webhook already fired.

What rollback promises is narrower: the shared copy stands where your branch does, and anyone who pulls from now on takes in the rolled-back line. Commits that reached the world stay reached — the branch simply stops pointing at them.

## Four pushes, one verb

Push is one verb wearing four different acts:

- **A branch with no shared copy** gets one created, with tracking set up in the same step. With several remotes, [`--to <remote>`](../reference/cli/push.md) names where it answers, once, because [one branch has one shared copy](branches.md#tracking-one-branch-one-shared-copy).
- **A branch whose shared copy stands behind it** gets that copy replaced, under the lease.
- **A branch whose shared copy was deleted** gets it put back, under a lease saying it must not exist. Telling a deleted copy apart from one that never existed is another reason fufu keeps a record of what it has sent.
- **A branch you undid** gets its shared copy rolled back.

`ff push --dry-run` previews the push without sending a remote update or moving local branches and files. The automatic fetch and maintenance can still run, so it is not a promise of no disk writes or network access.

## Shared-history policy

The rewrite verbs can rewrite commits that have already been pushed. They report published commits, then leave sending the rewrite to a separate `ff push`.

fufu has no branch-ownership check and no special force-push guard for `main`. A lease checks a ref's expected position, not who owns its commits or whether the team permits rewriting them. Enforce append-only shared history with team policy and server-side branch protection.

How work lands on the shared branch — merge commit, squash, rebase — stays the team's business and the forge's, not fufu's.

Pushed commits remain ordinary Git commits. That is [the invariant](invariant.md); it does not make published history immutable.
