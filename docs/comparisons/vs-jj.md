# fufu vs jj

Both tools snapshot working changes, support history editing, and provide operation-based recovery. fufu keeps a current Git branch and parks work per branch. jj works with a working-copy commit and bookmarks, and can record unresolved conflicts in commits. Those differences matter most when switching work, using Git tools, and continuing past a conflict.

The jj claims below use **jj v0.45.0** documentation: [Git compatibility](https://github.com/jj-vcs/jj/blob/v0.45.0/docs/git-compatibility.md), [bookmarks](https://github.com/jj-vcs/jj/blob/v0.45.0/docs/bookmarks.md), and [first-class conflicts](https://github.com/jj-vcs/jj/blob/v0.45.0/docs/conflicts.md). The comparison uses a colocated Git-backed jj workspace, the documented default for that version. The [performance page](../performance.md) records its own, different benchmark version.

<span id="what-jj-got-right-taken-wholesale"></span>

## Shared workflow ideas

fufu draws on jj's working-copy snapshots, change identity, descendant rewrites, and operation log. These do not mean every byte is always saved or every external effect can be undone. fufu's [snapshot coverage and retention](../concepts/snapshots-and-undo.md#coverage-and-limits) define its recovery scope.

## The architectural difference

In a colocated jj workspace, `.jj` and `.git` share a working directory. jj automatically imports and exports Git refs on commands; its operation store and conflict model add state beyond ordinary Git branch tips. Mixing jj and Git commands is supported, and imported Git ref changes can be undone through jj.

fufu stores its operation chains and open commits under `refs/fufu/`, with branch metadata and caches under the common Git directory. Git branches remain the checked-out lines of work. The [storage model](../concepts/invariant.md) and [architecture](../internals/architecture.md#where-fufus-state-lives) describe the added records, including what cannot be rebuilt after deletion.

## Side by side

| Task or state | jj v0.45.0, colocated | fufu |
| --- | --- | --- |
| Current work | A working-copy commit; Git HEAD is usually detached. | An open change above the current branch; a dirty capture writes an internal open commit without advancing branch history. |
| Names | Bookmarks follow rewrites of their target but there is no current bookmark that advances with each new commit. | The current Git branch advances on commit; automatically named branches are real refs from creation. |
| A conflicting replay | Records a logical conflict in the resulting commit; descendants can build on it. | Records a held rewrite for that branch; work can continue at its existing tip until resolution. |
| Other Git tools | Supported colocation, with documented limits for conflict representation, staging, and unfinished Git operations. | Ordinary attached branches and Git trees; fufu metadata and internal refs are additional visible state. |
| Outside Git changes | Automatically imports/exports refs; imports appear in the operation log. | Reconciles observed changes into foreign operations; uncaptured intermediate file states remain unavailable. |
| Change identity | Writes a `change-id` Git header by default; not all Git rewrites preserve it. | Uses the same header and derives IDs from SHAs for commits without it; [revision rules](../reference/revisions.md#commit-shas-and-change-ids) cover lookup and divergence. |
| Leaving the tool | Exported branches and ordinary commits remain usable in a colocated Git repository. jj-specific operation/conflict state still needs jj. | Ordinary branches remain usable in Git. Retain fufu refs and metadata to keep its recovery and parked-work records. |

<span id="what-the-inversion-buys"></span>

## Working with other tools

<span id="legibility"></span>

### Git views

fufu creates attached branches for normal switching and edit sessions. Its internal open commit is separate from the current branch tip, so Git status shows working edits while fufu can address the captured version. Tools that enumerate all refs can see the internal commits too.

jj's compatibility page documents the usual detached HEAD and the special storage of conflicted trees. These are workflow considerations, not a claim that Git tools are unusable with jj.

<span id="abandonability"></span>

### Leaving and returning

Removing a tool's executable differs from deleting its repository records. Neither tool's ordinary exported Git commits require that executable to read them. Deleting recovery records loses the recovery they provide. For fufu, removing refs may also leave snapshot or parked objects eligible for Git garbage collection. See [leaving and coming back](../concepts/two-regimes.md#leaving-and-coming-back).

<span id="git-fluency-keeps-paying"></span>

### Git habits

fufu retains current-branch behavior and Git-style revision suffixes, but changes defaults and selection rules. jj has its own revision language and bookmark workflow. Familiar command names do not imply equivalent behavior; use the [revision differences](../reference/revisions.md#differences-from-git-and-jj) and mappings below.

## What fufu gives up

- **Building on an unresolved replay result.** jj can rebase, merge, or back out commits that contain logical conflicts. fufu holds the requested rewrite instead, so the post-rewrite branch is unavailable until it can land.
- **Logical conflict propagation.** jj stores expressions rather than marker text. fufu's resolution replay carries markers; overlapping regions can stop a round and leave another hold.
- **Hunk-level selection.** fufu's commit and move commands select whole paths. It has no native interactive hunk picker or automatic hunk attribution; see the [FAQ](../faq.md#can-i-commit-some-hunks-of-a-file-and-leave-the-rest).

Both tools' operation recovery starts from records they actually made and retained. Adopting fufu does not make earlier editing sessions undoable.

## Two answers to conflicts

jj records a conflict in a resulting commit and lets you resolve it later. In fufu, a branch whose replay conflicts keeps its previous tip and receives a held-rewrite record. Earlier branches in a cascade may already have moved; capture, metadata, and fetch effects can also occur.

[`ff resolve`](../reference/cli/resolve.md) opens a resolution session with labeled markers, and [`ff done`](../reference/cli/done.md) lands a successful resolution. A parked-change arrival instead resolves in the working copy without done. The [conflict guide](../concepts/held-rewrites.md) covers both forms and undo counts.

[`ff push`](../reference/cli/push.md) blocks branches with held rewrites. jj v0.45.0 normally rejects outgoing conflicted commits but offers an explicit `--allow-conflicts` option, as its [push implementation](https://github.com/jj-vcs/jj/blob/v0.45.0/cli/src/commands/git/push.rs) documents. fufu has no equivalent logical-conflict object to send.

## Typing jj's words

These fufu aliases accept **fufu's** arguments and defaults:

| jj command | fufu command or alias | Important difference |
| --- | --- | --- |
| `jj new` | [`ff switch`](../reference/cli/switch.md), aliases `ff start`, `ff new` | Creates or continues a branch; bare starts at trunk and creates no empty history commit. |
| `jj bookmark` | [`ff branch`](../reference/cli/branch.md), alias `ff bookmark` | Manages Git branches; no bookmark subcommands. |
| `jj workspace` | [`ff worktree`](../reference/cli/worktree.md), alias `ff workspace` | Manages Git worktrees, each with a recovery log. |
| `jj squash` | [`ff absorb`](../reference/cli/absorb.md), alias `ff squash` | Defaults to moving open work into HEAD; selects paths, not hunks. |
| `jj rebase` | [`ff restack`](../reference/cli/restack.md), alias `ff rebase` | Uses the branch's recorded base; `--onto` changes it. |

[`ff log`](../reference/cli/log.md), [`ff show`](../reference/cli/show.md), [`ff diff`](../reference/cli/diff.md), [`ff describe`](../reference/cli/describe.md), [`ff edit`](../reference/cli/edit.md), [`ff restore`](../reference/cli/restore.md), [`ff undo`](../reference/cli/undo.md), [`ff evolog`](../reference/cli/evolog.md), and [`ff op`](../reference/cli/op.md) share names with jj. Read their help before translating a command: open-change, operation, and revision arguments differ.

`ff abandon` and `ff split` print guidance rather than performing an operation. Discard open work with `ff restore --all`, abandon a session with `ff done --abandon`, or use [`ff lift`](../reference/cli/lift.md) to return committed paths to the open change. [`ff commit <paths>`](../reference/cli/commit.md) records part and leaves the rest open. [Rewriting history](../guides/rewriting-history.md) supplies complete splitting recipes.

[`ff pull`](../reference/cli/pull.md) fetches **and reconciles local branches**, so it is not an exact equivalent of `jj git fetch`. [`ff git fetch`](../reference/cli/git.md) is Git passthrough. Sending with ff push also follows fufu's own [lease and seen-record rules](../concepts/push-boundary.md#push-carries-a-lease).

## Choosing

Choose based on the workflow you want: jj offers logical conflicts and work organized around commits and bookmarks; fufu offers current Git branches, per-branch parked work, and capture hooks for shells and agents. Try the [tutorial](../tutorial.md) and check [compatibility gaps](../faq.md) against your repository before adopting either workflow.

## The name

Jujutsu supplied both workflow ideas and naming inspiration. `ff` mirrors `jj` on the keyboard; “fu” refers to tool mastery. The [founding design](../internals/design.md) preserves the original account.
