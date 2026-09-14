# Pulling and pushing

<a id="the-push-boundary"></a>

[`ff pull`](../reference/cli/pull.md) updates local branches from their bases and remote copies. [`ff push`](../reference/cli/push.md) sends selected branches to the remote. Pulling can change your working copy; pushing changes what other people can fetch.

Suppose local `parser-fix` is based on `main` and has a remote copy at `origin/parser-fix`. These are separate relationships: `main` supplies the base for the work, while `origin/parser-fix` is the published version of that work. The [branch diagram](branches.md#base-branch-and-remote-copy) shows a larger example.

<a id="pull-is-the-incoming-half"></a>

## Pulling local updates

On `parser-fix`, bare `ff pull` fetches, updates local bases such as `main` from their remote copies, and brings `parser-fix` up to date with both its base and its own remote copy. Your commits replay on the updated history. Branches stacked above a moved branch can follow in the same run.

```sh
ff pull                     # Update the current branch and its bases.
ff pull parser-fix          # Select a branch from anywhere in the worktree.
ff pull --all               # Update all local branches, bases first.
```

Only the current branch's update changes the working copy. Updates to other branches move refs and create commit objects.

For a remote copy, pull distinguishes two cases:

- If you have not changed the local branch since fufu last recorded its remote position, the branch follows the remote copy, including a remote rewrite.
- If you have changed it locally, pull takes in new remote work and replays your work on top. It uses recorded local rewrites and pushed tips to distinguish new work from older versions of your own commits that a later push can replace.

A conflicting replay leaves that branch [held](held-rewrites.md) at its existing tip, without putting a partially replayed tree in your working copy. Earlier successful updates can remain, and the run can update other branches. [`ff resolve`](../reference/cli/resolve.md) starts resolution when you are ready.

## Fetching and dry runs

Fetching writes downloaded objects and remote-tracking refs such as `origin/parser-fix`. These tracking refs are local observations of the remote; refreshing them alone does not replay your local branches. Foreground pull also fetches tags and prunes deleted tracking refs. Automatic fetch leaves tags to foreground pull.

`ff pull --dry-run` still fetches and previews the local replay without changing local branches or working-copy files. `ff pull --dry-run --no-fetch` uses the existing tracking refs instead. Neither form sends branch updates to the remote. Automatic trimming and update maintenance can still run after a successful invocation, even with both flags.

Pull's recorded local branch and file updates form one [undoable operation](snapshots-and-undo.md). Fetched objects, tracking-ref updates, tags, and ambient maintenance are separate from that undo step.

<a id="four-pushes-one-verb"></a>

## Choosing what to push

Bare `ff push` sends the current branch. `ff push parser-fix parser-tests` sends only those branches, each to its own remote copy. Bases and children are not included automatically, and there is no `--all` option. No other fufu command pushes as a default side effect.

Named pushes preserve each target's parked work. Push notes belong to the branch where the command ran and name the branch sent. If an earlier off-branch push in v0.16.0 affected your saved work, see [recovery after a named push](../guides/stacked-changes.md#inspect-local-work-after-a-named-push).

| Remote copy | What push does |
| --- | --- |
| None yet | Creates it and sets tracking. |
| Exists | Updates it to the selected local branch, subject to the checks below. |
| Was deleted | Recreates it only if the remote ref is still absent. |
| Contains work you undid locally | Can roll it back under the same lease and server-side rules. |

With one remote, or one named `origin`, the first push selects it. `ff push --to upstream` selects a remote for a branch without a remote copy and records that choice. A branch already tracking a copy on another remote is refused; fufu models [one remote copy per branch](branches.md#tracking-one-branch-one-remote-copy).

`ff push --dry-run` previews without sending a remote update or moving local branches and files. Push can auto-fetch before planning on `fufu.autoFetch`'s cadence; `--no-fetch` skips that fetch. Dry-run flags do not disable all network access or maintenance.

## Push carries a lease

A **lease** means the remote ref must still match its expected value when the server updates it. For example, if a push plans to replace `origin/parser-fix` at commit A, but another person moves it to B first, that update is refused.

Replacing remote commits also requires fufu's **seen record** to agree with the remote-tracking tip. The seen record is the remote position fufu recorded through pull, [`ff switch`](../reference/cli/switch.md) onto a remote branch, [`ff clone`](../reference/cli/clone.md), or a successful push. A fetch by an editor, [`ff git fetch`](../reference/cli/git.md), automatic fetch, or `ff pull --dry-run` refreshes tracking refs without updating that record. Fetching alone therefore does not authorize replacing a newly observed remote tip.

A fast-forward adds commits without removing existing ones and can proceed without seen-record agreement. Creating or recreating a remote copy uses a lease requiring the ref to be absent. The server checks the lease even if the earlier local checks passed, catching races after planning.

If a lease or seen-record check refuses a branch, that branch is not sent. Pull and review the remote changes before retrying. In a multi-branch push, other branches can still succeed; read each branch's report. A held rewrite also blocks that branch's push.

## Rollback is undo, then push

[`ff undo`](../reference/cli/undo.md) restores local recorded state. It does not send a remote update. To roll back a pushed commit, undo the commit locally, use [`ff status`](../reference/cli/status.md) to check the result, then push that branch again. If other operations followed the commit, use [history and recovery commands](snapshots-and-undo.md#choosing-a-recovery-command) to select the intended local state first.

The new push can move the remote copy back to the local tip because fufu records the tips it previously sent. It still uses a lease and must satisfy the server's rules. A changed remote tip can refuse the rollback.

### What rollback promises

A successful rollback makes the remote branch point at the selected local commit. It does not erase commits from other clones or reverse CI runs and webhooks. The remote update is a new action, not an undo of those effects.

<a id="published-history-is-append-only"></a>
## Shared-history policy

Local rewrite commands accept already-pushed commits and report them. Sending that rewrite requires a separate push.

fufu has no branch-ownership check and no special force-push protection for `main`. A lease checks a ref's position, not who owns the commits or whether a team permits replacing them. Use team policy and server-side branch protection to enforce append-only shared history.

How reviewed work reaches the shared branch — a merge commit, squash, or rebase — remains the team's and forge's choice.
