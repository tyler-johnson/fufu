Send the current branch to its remote copy. With branch names, send exactly those branches instead. Each update uses a lease: the remote ref must still match the expected value. `ff publish` is an alias. Remote updates cannot be reversed by `ff undo`.

## Examples

```sh
ff status                       # Review outgoing work
ff push -n                      # Preview; fetching can still run
ff push                         # Send this branch
ff push parent child            # Send both named branches
ff push --to upstream           # Choose and remember a remote
```

### Options

### Branches and remotes

Names accept unique local branch prefixes. Bases and dependents are not sent automatically; name every branch you want to send. There is no `--all`.

Current off-branch push notes can replace a named target's open state with this checkout's tree. Prefer pushing each branch while current. If already affected, inspect retained pre-push captures before recovering parked edits; see the [stack guide](../../guides/stacked-changes.md).

A branch without a remote copy gets one with tracking configured. An upstream named after another branch, such as feature tracking origin/main, is treated as its base: push creates origin/feature and records origin/main as the base. A previously deleted copy is recreated with a lease requiring that it remain absent.

`--to <remote>` chooses a remote for an unassigned branch and remembers it for later commands. A branch already assigned elsewhere refuses it. With one remote, or a remote named origin, the flag is usually unnecessary.

`--dry-run` previews without sending remote updates, moving local branches or files, or recording the push. It skips the push's capture. Automatic fetching and maintenance can still run; `--no-fetch` skips fetching.

### Leases, holds, and refusals

Replacing remote commits requires agreement between the tracking tip and fufu's seen record. `ff pull`, switching onto a remote branch, cloning, and successful pushes update that record. A fast-forward can proceed without the agreement because it removes no commits. A background fetch or pull dry run updates tracking refs without updating the seen record.

Push itself can auto-fetch on `fufu.autoFetch`'s cadence. Fetching does not authorize replacing an unseen remote tip; the wire lease also catches changes made after planning. A refused lease requires inspecting and incorporating the remote state before trying again.

A held rewrite blocks that branch's push. In a multi-branch run, other branches still proceed. Refusals make the final exit 1; a held branch makes it 3 unless a refusal makes it 1. A successful branch update is not rolled back when another fails.

### Shared history and rollback

There is no branch-ownership check or special protection for main. Local rewrites of pushed commits are allowed, and push can send them when its guards pass. Team policy and server-side protection determine whether that is appropriate.

To reverse a remote branch update, restore the desired local history and push again under the same lease and server rules. This does not erase commits from other clones, CI runs, or webhooks.

### Report and JSON

The current branch is reported first when selected, then each other named branch and its result. JSON's `branches` array includes each branch's `push`, `pushed`, and `error`. Top-level `push` and `pushed` describe the current branch; they are `NotNamed` and false when it was not selected.

`push/unrecorded` means a remote send succeeded but local bookkeeping failed. The command can stop before its final multi-branch report. A retry that finds nothing to push does not reconstruct a missing push note or published pointer; inspect the remote and operation log, and use `ff explain push/unrecorded` for the repair limits.
