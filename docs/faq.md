# FAQ

## Is my repository still a normal git repository?

Yes. fufu uses Git objects, attached branches, and worktrees. It also adds internal refs and metadata that tools listing all refs can see. Raw Git can still leave a detached HEAD or unfinished operation; fufu reports those states. See [Git storage model](concepts/invariant.md).

## Can I stop using fufu? Can I use it on one machine and not another?

Yes. Ordinary branches and commits remain usable through Git. Keep fufu's refs and metadata if you want its parked work and recovery history later: deleting those records can remove the last references to data that Git may then garbage-collect. [Leaving and coming back](concepts/two-regimes.md#leaving-and-coming-back) covers both cases.

## What happens when a teammate force-pushes or rewrites history I've built on?

[`ff push`](reference/cli/push.md) checks an expected remote-tip lease. Replacing remote commits also requires seen/tracking agreement. [`ff pull`](reference/cli/pull.md) then follows or replays onto the remote according to the recorded divergence; replay can drop superseded or empty commits and reports them. See [pulling and pushing](concepts/push-boundary.md) and the [force-push recovery example](guides/recovery.md#someone-force-pushed-over-my-branch).

## Does fufu work with GitHub, GitLab, and other forges?

It uses Git remotes without a forge-specific integration. Native clone/fetch reads credential helpers and `url.insteadOf`, but does not honor `http.proxy`; push uses Git's transport. See [network settings](reference/config.md#what-fufu-reads-from-gits-config).

Special review refspecs, such as Gerrit's `refs/for/main`, require [`ff git push`](reference/cli/git.md) under `coach` or `observe` policy. That review flow is untested.

## Does fufu work with git LFS?

LFS-dependent repositories are unsupported. fufu's native snapshot and checkout paths do not implement a tested LFS workflow. [Substrate limits](internals/substrate.md#the-git-free-destination) describe the boundary.

Separately, snapshots skip oversized content under `fufu.maxFileSize` (50 MiB by default), including oversized modifications to tracked files. Older index/base content may remain in the snapshot. See [capture limits](concepts/snapshots-and-undo.md#coverage-and-limits).

## Does fufu work with submodules?

There are no native submodule commands, and submodule workflows are untested. `ff git submodule …` runs Git, but does not establish that fufu snapshots or restores nested working changes. See [substrate limits](internals/substrate.md#the-git-free-destination).

## Where does fufu keep its state, and how big does it get? What does `ff op trim` do?

Repository recovery records live under `refs/fufu/`; caches and branch metadata live under `<common-dir>/fufu/`. Normal branch pushes do not send these refs. [Architecture](internals/architecture.md#where-fufus-state-lives) maps the paths and distinguishes rebuildable caches from retained records.

[`ff op trim`](reference/cli/op-trim.md) shortens the current and orphan removed-worktree logs according to `fufu.keep` (90 days by default). Automatic trim runs at most daily by default; manual trim also attempts `git gc --auto`, even when it drops nothing. Retention is an age policy, not a fixed disk-size bound, and the last pre-trim chain stays in trash. See [retention](guides/recovery.md#retention-and-the-earliest-recovery-point).

## Why is there no staging area? I liked the staging area.

fufu uses the working copy as the open change. [`ff commit <paths>`](reference/cli/commit.md) selects files or directories when recording a commit and leaves the remainder open. Git's index still exists for interoperability and hooks, but its staged selection does not control ff commit. See [partial commits](concepts/changes.md#partial-commits).

## Can I commit some hunks of a file and leave the rest?

Not with a native fufu command. Use `ff git commit -p` for Git's interactive picker under `coach` or `observe` policy. Do not follow `ff git add -p` with ff commit: that commits working-copy content rather than the staged selection. See [same-file splitting](guides/rewriting-history.md#paths-and-hunk-limits) for a complete recipe.

## Why can't `ff undo` take back a push?

[`ff undo`](reference/cli/undo.md) restores recorded local state. It cannot reverse remote updates, another clone's fetch, CI, or webhooks. A subsequent push is a new remote update, checked against a lease and server policy. See [rolling back a push](concepts/push-boundary.md#rollback-is-undo-then-push).

## What does strict mode refuse?

Through ff git, `fufu.gitPolicy=strict` refuses recognized Git writes such as commit, reset, stash, rebase, and push, including tag pushes. It runs no replacement command. Reads and commands such as bisect and submodule pass through, and so does `ff git merge`, for a fast-forward into a local branch; [`ff merge`](reference/cli/merge.md) is fufu's own merge. [Git policy](reference/config.md#gitpolicy) owns the rules and exceptions.

The passthrough checks policy before capture, while agent hooks attempt capture first. Only Claude Code emits pre-tool denial replies; the other installed agent adapters capture and tally without denying tools. See [agent policy](agents/setup.md#pick-a-git-policy).

## How far back can undo reach? What about before I ran `ff init`?

Undo reaches the earliest retained recovery point on the current worktree's log. [`ff init`](reference/cli/init.md) starts that log from observed state; it cannot reconstruct earlier editing sessions. Retention can move the earliest point forward. See [coverage and limits](concepts/snapshots-and-undo.md#coverage-and-limits).

## Does fufu run my git hooks?

Yes. fufu resolves the four commit-time hooks through `core.hooksPath` and runs them itself. The table describes a nonempty operation reaching its hook phase; initial refusals and no-ops may stop earlier.

| Operation | pre-commit | prepare-commit-msg / commit-msg | post-commit |
| --- | --- | --- | --- |
| ff commit, including a path selection | Yes | Yes | Yes |
| [`ff absorb`](reference/cli/absorb.md) or [`ff lift`](reference/cli/lift.md), open change included in `--from` | Yes, when selected open content differs from HEAD | Only with `-m` targeting a closed commit | No |
| Absorb or lift, closed sources only | No | Only with `-m` targeting a closed commit | No |
| Absorb or lift into the open change | No new worktree content is committed | No; `-m` sets its pending message | No |
| [`ff done`](reference/cli/done.md), edit-session landing | Yes | When landing a changed description | No |
| ff done, resolution landing | Yes, once before applying the resolution | A resumed done intent can still run message hooks for its changed description; absorb/lift resolution does not rerun its message hooks | No |
| [`ff describe <rev>`](reference/cli/describe.md) | No | Yes | No |
| ff describe, open-change message | No | No; hooks wait until commit | No |
| [`ff restack`](reference/cli/restack.md), ff pull, [`ff fold`](reference/cli/fold.md) | No | No | No |

`--no-verify` skips `pre-commit` and `commit-msg`, but not `prepare-commit-msg`. A failing gate aborts the operation; `post-commit` is a notification and its failure does not undo a commit. For a pre-commit hook, fufu stages the selected content provisionally and re-scans formatter edits afterward. [Hook implementation details](internals/substrate.md#behavioral-compatibility) cover the index and message file; the command references cover each mode.

## Do I need git installed?

Install Git for a complete setup. Local core operations are native, but push, Git passthrough, filesystem-remote upload-pack, and a narrow fetch fallback require Git programs. Network authentication, signing, and hooks may require additional helpers. No minimum Git version is declared. See [installation dependencies](install.md) and the [execution table](internals/substrate.md#the-execution-ladder-as-it-stands).

## What's the name about?

“Fu” refers to tool mastery, doubled in the style of jj. `ff` mirrors `jj` on the keyboard. The [founding design](internals/design.md) also connects the name to Japanese fūfu (a married couple) and the West African dish.
