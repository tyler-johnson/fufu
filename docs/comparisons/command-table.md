# Command table

Find a task, then use the linked command page for its defaults and options. These are workflow mappings; the commands do not necessarily change the same state. Git examples use the [Git 2.50.1 manuals](https://github.com/git/git/tree/v2.50.1/Documentation).

Repository commands normally attempt a snapshot before acting, subject to their guards and [capture limits](../concepts/snapshots-and-undo.md#coverage-and-limits). Recovery requires retained records. [`ff git <args>`](../reference/cli/git.md) runs Git verbatim after policy checks and a capture attempt; see [Using fufu alongside Git](../concepts/two-regimes.md).

## Set up and inspect

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| Create or adopt a repository | `git init` | [`ff init`](../reference/cli/init.md) | Also enables snapshots and creates the earliest recovery point; can adopt an existing checkout. |
| Clone | `git clone <url>` | [`ff clone <url>`](../reference/cli/clone.md) | Enables snapshots on arrival. Native HTTP fetch does not honor `http.proxy`; see [network settings](../reference/config.md#what-fufu-reads-from-gits-config). |
| Inspect current work | `git status` | [`ff status`](../reference/cli/status.md) | Shows the open change, held rewrites, and replay predictions against the base and remote copy. |
| Read a patch | `git diff` | [`ff diff`](../reference/cli/diff.md) | Shows the open change, including eligible untracked content. Use `ff git diff <a> <b>` for two revisions. |
| Read commit history | `git log` | [`ff log`](../reference/cli/log.md) | Includes an open-change row. The letters column is a **change ID**, not an operation ID; the hexadecimal column is a commit SHA. |
| Follow a file | `git log --follow -- <file>` | `ff log <file>` | Follows renames by default; revisions go after `-r`, and narrowing with `-r` disables rename following. |
| Inspect a commit | `git show <rev>` | [`ff show <rev>`](../reference/cli/show.md) | Bare shows the open change. Operations use [`ff op show`](../reference/cli/op-show.md); syntax belongs to [Revisions and IDs](../reference/revisions.md). |
| Find recent branches and parked work | `git branch -v`, `git stash list` | Bare `ff`, or [`ff map`](../reference/cli/map.md) | Draws a compact branch graph and parked changes; fufu parks are not Git stash-list entries. |

## Commit and switch work

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| Commit current work | `git add` then `git commit -m <msg>` | [`ff commit -m <msg>`](../reference/cli/commit.md) | Records the working copy, not a hand-staged index. |
| Commit selected files | `git add <paths>` then `git commit` | `ff commit <paths>` | Files or directory prefixes only; no globs or hunks. The remainder stays open. |
| Commit selected hunks | `git add -p` then `git commit` | `ff git commit -p` | No native fufu hunk picker. Git's interactive commit requires `coach` or `observe` policy; `ff git add -p` followed by `ff commit` does not preserve the selection. |
| Create a branch and switch | `git switch -c <new> [<base>]` | [`ff switch <base> -b <new>`](../reference/cli/switch.md) | Bare `ff switch` creates an automatically named branch at trunk. `ff start` and `ff new` are aliases. |
| Switch branches | `git switch <branch>` | `ff switch <branch>` | Parks the open change with the branch left and resumes the target's. A remote-only branch can be created locally and tracked. |
| Save work for later | `git stash push -u`, then `git stash pop` | `ff switch` away, then back | One parked change per branch. Staged distinctions do not survive parking; [parking and resuming](../concepts/changes.md#parking-and-resuming) covers moved tips and conflicts. |

## Rewrite and combine commits

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| Add working changes to a commit | `git commit --amend`, or fixup plus autosquash | [`ff absorb`](../reference/cli/absorb.md) | Defaults to the commit below the open change; `--into <rev>` selects another target. Paths select files, not attributed hunks. |
| Squash a run | `git rebase -i` with squash/fixup | `ff absorb --from HEAD~2..HEAD` | Moves the last two commits into their predecessor. If that target is on trunk, name it explicitly with `--into`. Sources must form a contiguous first-parent run; emptied sources are reported. |
| Reword a commit | `git commit --amend -m`, or rebase reword | [`ff describe <rev> -m <msg>`](../reference/cli/describe.md) | Reparents descendants. Without a revision, edits the pending message instead of branch history. |
| Edit an earlier commit | `git rebase -i` with edit | [`ff edit <rev>`](../reference/cli/edit.md), then [`ff done`](../reference/cli/done.md) | Opens an attached session branch; done amends, replays, and returns. |
| Return committed content to current work | `git reset --soft HEAD~` | [`ff lift`](../reference/cli/lift.md) | Moves content into the open change and leaves files in place, but rebuilds the index and may replay descendants. It is not a soft-reset equivalent. |
| Move content between commits | Interactive rebase and patch editing | `ff lift --from <rev> --into <target>` | Absorb and lift share `--from`, `--into`, paths, and `-m`; their defaults differ. See [rewriting recipes](../guides/rewriting-history.md). |
| Replay onto a base | `git rebase <base>` | [`ff restack`](../reference/cli/restack.md) | Uses the recorded base, otherwise trunk; descendants follow with [cascade exceptions](../concepts/branches.md#the-cascade). May auto-fetch first. |
| Change the base | `git rebase --onto <base> …` | `ff restack --onto <base>` | Records the new base for later restacks. A conflicting branch holds; earlier successful branches can already have moved. |
| Land a feature locally | Rebase, switch, fast-forward merge, branch delete | [`ff fold [<target>]`](../reference/cli/fold.md) | Replays onto the target (default trunk), advances it, deletes the source, and switches there. `--stay` keeps the source checkout. |
| Resolve a deferred replay | `git rebase --continue` | [`ff resolve`](../reference/cli/resolve.md), edit markers, `ff done` | A rewrite resolution uses a session; overlapping conflicts can require another round. A [parked-change arrival](../concepts/held-rewrites.md#parked-change-arrival) resolves in place without done. |

## Fetch and push

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| Fetch without replaying local branches | `git fetch` | `ff git fetch` | Runs Git's transport. Updating tracking refs does not update fufu's seen record. |
| Fetch and update current work | `git pull --rebase` | [`ff pull`](../reference/cli/pull.md) | Reconciles the current branch with its remote copy and required local bases; names or `--all` widen selection. Not just a fetch. |
| Preview a pull | Fetch, then inspect | `ff pull --dry-run` | Skips local replay but still fetches objects, tracking refs, and tags unless `--no-fetch`; [dry-run effects](../concepts/push-boundary.md#fetching-and-dry-runs) include maintenance. |
| Send a branch | `git push`, `--force-with-lease`, `-u` | [`ff push`](../reference/cli/push.md) | Current branch by default; `--to <remote>` records the destination. Replacements require seen/tracking agreement and an expected remote-tip lease. No branch-ownership check. |
| Send named branches | `git push <remote> <refspec>…` | `ff push <branch>…` | Each send has its own result. Review the current [off-branch open-state issue](../guides/stacked-changes.md#inspect-local-work-after-a-named-push) before using this form. |

## Manage branches, worktrees, and remotes

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| List branches | `git branch` | [`ff branch`](../reference/cli/branch.md) | Includes parked work and remote-only branches; `--all` removes display limits. |
| Create without switching | `git branch <name> [<rev>]` | `ff branch <name> [<rev>]` | Defaults to trunk, not HEAD. `@` uses HEAD and copies the open change. |
| Rename current branch | `git branch -m <name>` | `ff describe -b <name>` | Carries branch metadata and fufu pointers along with the name. |
| Delete a branch | `git branch -d` / `-D` | `ff branch -d <name>` | No merged-history check; records local recovery. `--shared` also deletes the remote copy under a lease, which undo cannot reverse. |
| Add another checkout | `git worktree add <path>` | [`ff worktree <path>`](../reference/cli/worktree.md) | Creates its recovery log; default branch name comes from the directory. Refuses a branch checked out elsewhere. |
| List remotes | `git remote -v` | [`ff remote`](../reference/cli/remote.md) | Add one with `ff git remote add <name> <url>`. |

## Restore and recover

| Task | Git | fufu | Qualification |
| --- | --- | --- | --- |
| Discard current file edits | `git restore <path>` | [`ff restore <path>`](../reference/cli/restore.md) | Defaults to HEAD, whereas Git restore defaults to the index. Only working files change; the index and branch stay put. |
| Restore another version | `git restore --source=<rev> <path>` | `ff restore --from <rev> <path>` | Also accepts retained operation/time sources with `--at-op` or `--at`. |
| Discard all current edits | `git reset --hard` | `ff restore --all` | Restores working files to HEAD and removes eligible newly created files. Does **not** reset refs or the index; not a replacement for `git reset --hard <rev>`. |
| Recover a recorded local state | Reflog plus reset/restore | [`ff history`](../reference/cli/history.md), [`ff undo`](../reference/cli/undo.md) | Follows the current worktree's undo steps, restoring recorded refs, index, and files within [recovery limits](../guides/recovery.md#what-undo-cannot-reach). |
| Select a particular recorded state | Reflog lookup | [`ff op restore <op>`](../reference/cli/op-restore.md) | Operation IDs are retained hexadecimal addresses. [`ff op log`](../reference/cli/op-log.md) lists them; [`ff op diff`](../reference/cli/op-diff.md) compares their file trees. |
| Invert one commit | `git revert <rev>` | `ff git revert <rev>` | [`ff op revert`](../reference/cli/op-revert.md) instead inverts still-applicable **ref transitions of an operation**, preserving files/index and HEAD selection. |
| Other Git work | Cherry-pick, merge, bisect, plumbing | `ff git <args>` | Git supplies the behavior and streams; policy may refuse recognized writes. |

<span id="notes"></span>

See [Revisions and IDs](../reference/revisions.md) for syntax, [recovery](../guides/recovery.md) for choosing a restore command, and [configuration](../reference/config.md#gitpolicy) for passthrough policy.
