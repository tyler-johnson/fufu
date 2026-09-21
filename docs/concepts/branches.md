# Branches and parked work

<a id="branches"></a>

Use a branch for each task. [`ff start`](../reference/cli/switch.md) creates one from trunk, the repository's main development branch. [`ff switch`](../reference/cli/switch.md) returns to an existing branch. Both commands park your [open change](changes.md) on the branch you leave and restore any work waiting at the destination.

## Creating and switching

```sh
ff start -b parser-fix
# Edit files for this task.
ff switch main
ff switch parser-fix
```

The last command brings your parser edits back. You do not need to commit them just to switch tasks.

`ff start` and `ff switch` are spellings of the same command. With no target they create a branch from trunk. An existing branch target resumes it; a revision target creates a branch at that commit. To create a new branch from an existing one, use `ff start main -b parser-fix`.

A new branch normally opens with no uncommitted work. `ff start @ -b experiment` instead copies the current open change onto a new branch at the commit beneath it. The original branch keeps its own copy. `ff start -m "investigate parser options"` gives new work a pending commit message when you begin.

## Switching by prefix

A unique branch-name prefix is enough: `ff switch uni` selects `unicode-cleanup` if nothing else matches. An ambiguous prefix is refused with the candidate names.

You can also switch to a branch that exists only on a remote. `ff switch spike` or `ff switch origin/spike` creates a local `spike` that tracks `origin/spike`. Adding `-b my-spike` creates a separate branch based on that target instead.

<a id="minted-names"></a>

## Automatically named branches

If you omit `-b`, a new branch gets a generated name such as `ff/hidden-wren`. It is already an ordinary Git branch under `refs/heads/`, visible to Git commands and GUIs. Work is not genuinely unnamed or waiting outside the branch system. Some command output calls these branches *anonymous* or their generated names *petnames*.

You can keep the generated name while you work and choose a descriptive name when you are ready. `ff start -b hotfix` chooses it at creation time.

<a id="claiming-a-name"></a>

## Renaming a branch

[`ff describe -b parser-fix`](../reference/cli/describe.md) renames the current branch, whether its previous name was generated or chosen. `-b` changes the branch name; `-m` changes the open change's pending commit message.

The fufu rename carries its branch-associated records and open-change ref to the new name. A raw `git branch -m` renames Git's branch but does not perform that metadata update. Use the fufu command to keep parked work and its description associated with the renamed branch.

## Every commit lands on a branch

[`ff commit`](../reference/cli/commit.md) records work on the current branch and advances its tip. Selecting an old revision through `ff switch` creates a branch there rather than detaching HEAD. Editing a historical commit also uses a temporary branch. These are ordinary Git branches; see [using fufu alongside Git](two-regimes.md) for how other tools see them.

## Listing and deleting

[`ff branch`](../reference/cli/branch.md) lists chosen names first, then generated names. It shows branch tips, parked work, pending descriptions, and the relation to remote copies. `ff branch parser-fix` creates a branch at trunk without switching to it; an additional revision chooses a different starting point.

`ff branch -d parser-fix` deletes a local branch without requiring it to be merged. Its tip and parked work remain recoverable from the recorded operation while retained; [`ff undo`](../reference/cli/undo.md) restores the deletion. Deleting a local branch leaves its remote copy in place.

`ff branch -d parser-fix --shared` also deletes the remote copy under a [lease](push-boundary.md#push-carries-a-lease). Undo cannot restore that remote deletion.

## Base branch and remote copy

A branch can have two separate relationships. Its **base branch** is the line of development its commits build on. Its **remote copy** is the published version of the same branch. For example:

```text
Local branches                  Remote copies on origin

main                            origin/main
  └── parser-fix                origin/parser-fix
        └── parser-tests        origin/parser-tests

parser-fix is based on main.
parser-tests is based on parser-fix.
origin/parser-fix is the remote copy of parser-fix.
```

Trunk is the default base. fufu uses `fufu.trunk` when configured, otherwise repository heuristics; ambiguous choices are reported for you to resolve.

## Stacking: a branch records its parent

`ff start parser-fix -b parser-tests` creates a branch at `parser-fix`'s tip and records `parser-fix` as its base. A bare `ff start` starts from trunk without an explicit base record, so it follows the repository's trunk setting.

[`ff restack`](../reference/cli/restack.md) replays a branch's commits onto its base's current tip. `ff restack --onto main` changes the base to `main` and replays the work there. [Stacked changes](../guides/stacked-changes.md) walks through a stack in review.

A merge inside a replayed range is carried: its parents are mapped through the replay and re-merged, and its own change, what [`ff show`](../reference/cli/show.md) measures against the auto-merge of its parents, is laid over the result. A merge that resolved a conflict carries the resolution when its mapped parents conflict the same way; when they conflict differently, the replay holds at the merge with the fresh conflict, and the old resolution stays in the merge's own commit to read. A merge that only took an older base in ends up with that base beneath its other parent and flattens away; what it resolved or edited survives as an ordinary commit, and the report names it under `flattened`. A merge with nothing of its own is dropped as empty. [`ff merge`](../reference/cli/merge.md) makes such a merge on purpose, for a branch you don't own, and never for the base.

### Parent inference for branches made with Git

For a branch created outside fufu, fufu can infer a base when its tip matches exactly one other non-trunk branch, or Git's reflog identifies the branch it was created from. Otherwise the base defaults to trunk; `ff restack --onto` supplies a correction.

An upstream with a different branch name, such as `origin/main` for local `parser-fix`, is treated as a base rather than as the remote copy of `parser-fix`. The first [`ff push`](../reference/cli/push.md) records that base when it creates the same-named remote copy and sets tracking.

### The cascade

Rewriting a branch can also replay the local branches based on it. This is a **cascade**: parents update before children, inside the original command's operation, so one undo takes back the rewrite and its cascade together.

The commands that run cascades are `ff restack`, [`ff fold`](../reference/cli/fold.md), [`ff pull`](../reference/cli/pull.md), [`ff absorb`](../reference/cli/absorb.md), [`ff lift`](../reference/cli/lift.md), `ff describe <rev>`, and [`ff done`](../reference/cli/done.md).

A conflicting replay leaves that branch [held](held-rewrites.md) at its existing tip. Branches above it stay put; successful updates elsewhere in the cascade can stand. Resolving that branch and finishing with `ff done` resumes the cascade from there.

Branches checked out in another worktree or already holding a rewrite are skipped and named. A branch with no commits of its own stays put. Read the report for what updated, held, or was skipped; [conflict reporting](held-rewrites.md#deferred-requires-loud) explains the exit-code distinctions.

<a id="pull-reaches-a-branch-and-what-it-answers-to"></a>

## Updating branches

`ff pull` updates the current branch against both its base and remote copy, bringing local bases up to date first. `ff pull parser-fix parser-tests` selects branches by name; `ff pull --all` updates all local branches in base-first order. Only the current branch's working copy changes; the other updates move refs and create commit objects.

[Pulling and pushing](push-boundary.md) explains fetch behavior, dry-run effects, leases, and rollback.

<a id="tracking-one-branch-one-shared-copy"></a>

## Tracking: one branch, one remote copy

fufu tracks one remote copy per branch. With a single remote, or one named `origin`, the first push creates the copy and sets tracking. With several remotes, `ff push --to upstream` selects one and remembers it. It is refused if the branch already tracks a copy on a different remote.

A remote-tracking ref such as `origin/parser-fix` is your local record of the remote branch's last fetched position. Automatic fetching refreshes these refs on `fufu.autoFetch`'s cadence, ten minutes by default, without replaying local branches. `--fetch` requests a fetch now and `--no-fetch` skips it; `ff pull` performs the local updates.

## Pruning deleted remote branches

After a forge deletes a merged branch's remote copy, `ff branch --prune` fetches and can remove its local branch. It looks for configured upstreams whose tracking refs are gone and whose remote copies were previously recorded. It keeps and reports branches with commits the remote copy never held, the current branch, branches checked out in other worktrees, and held branches. Children of a pruned branch are redirected to that branch's base.

The local deletions form one undoable operation. `--dry-run` previews them but still fetches; `--no-fetch` uses existing tracking refs. Maintenance can still run. `fufu.pruneGone` enables this pruning within `ff pull`; see [configuration](../reference/config.md) for the setting.
