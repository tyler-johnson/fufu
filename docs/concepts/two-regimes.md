# Using fufu alongside Git

<a id="the-two-regimes"></a>

fufu works in an ordinary Git repository. Your branches and committed history remain available to Git, GUIs, IDEs, CI, and teammates who do not use fufu. You can use both tools in the same repository; what happens to uncommitted work depends on which command or integration runs.

<a id="inside-through-fufu"></a>

## Commands through fufu

Use fufu commands when you want its working-copy workflow: [`ff commit`](../reference/cli/commit.md) records files without staging, and [`ff switch`](../reference/cli/switch.md) parks uncommitted work with the branch you leave. [Working copy and commits](changes.md) explains that lifecycle.

Repository commands and active hooks take [snapshots](snapshots-and-undo.md), subject to successful recording, coverage, and retention. [`ff undo`](../reference/cli/undo.md) restores recorded local state on the current worktree's chain. [Held rewrites](held-rewrites.md) let you defer conflicting replays until you choose to resolve them.

<a id="outside-around-fufu"></a>

## Commands through Git and other tools

An IDE commit button, a GUI branch switch, or a raw Git command performs that tool's operation. A Git branch switch does not park work through fufu, and a Git rebase can stop at a conflict. Finish or abort a Git merge, rebase, or bisect with Git; fufu does not take over its in-progress session.

Git's index still exists. If you stage files with Git, a later fufu commit selects working-copy files rather than using that staged selection. A fufu branch switch restores parked edits as unstaged work. Use [`ff diff`](../reference/cli/diff.md) and [`ff status`](../reference/cli/status.md) to check the files before committing.

Git can read the extra objects fufu stores for snapshots, open changes, and resolution sessions. Views such as `git log --all` can show those refs as well as ordinary branch history. See the [storage model](invariant.md) for their layout.

### Which program ran

[`ff git`](../reference/cli/git.md) runs Git after attempting a snapshot, unless `fufu.gitPolicy=strict` refuses the command first. A permitted command still has Git's semantics. A snapshot failure prints a warning and Git runs anyway. Strict refusals can write the policy tally even though Git and the pre-command snapshot do not run.

The shell alias `alias git='ff git'` routes typed Git commands through that path. An editor or script usually bypasses shell aliases. Installed agent hooks instead act on the events the client delivers; they attempt a snapshot before their policy check. See [hook setup](../reference/hooks/index.md) for activation and [configuration](../reference/config.md) for policy settings.

[`ff push`](../reference/cli/push.md) blocks a branch with a held rewrite. A raw Git push that bypasses fufu has no such guard; an active alias or agent hook may refuse it under strict policy. Server-side protection and the [team's shared-history policy](push-boundary.md#shared-history-policy) apply independently.

<a id="lazy-absorption"></a>

## Returning after outside changes

At the next reconciliation, fufu compares its recorded state with the repository. Observed ref changes enter the operation log as **foreign operations**, accompanied by available Git reflog messages. Status reports the outside changes; its notice remains while the log's current entry is foreign.

Several Git commands between observations can become one foreign operation. Recovery points exist only for recorded states at the observed endpoints. Reflogs record ref movements, not each intermediate working copy; neither reconciliation nor undo can reconstruct uncaptured edits that an outside command discarded. The [snapshot coverage and limits](snapshots-and-undo.md#coverage-and-limits) apply here too.

If a branch tip moved while work was parked on it, switching back replays that work over the new tip and can produce a [held arrival](held-rewrites.md#parked-change-arrival). Use [`ff describe -b`](../reference/cli/describe.md) for branch renames so fufu moves its branch-associated records too; raw Git renames do not perform that update.

Commits made outside fufu may lack a `change-id` header. Their derived IDs depend on their hashes, so an outside rewrite that drops the header may also change the ID. fufu can report the ref movement without knowing the same rewrite relationships it records for its own commands.

<a id="a-weekend-without-fufu"></a>

## Leaving and coming back

You can use Git for a session or remove the fufu binary. Removing the binary does not delete repository objects: branches, commits, snapshot refs, and parked changes remain in Git. The fufu commands that interpret and manage those records are no longer available until you reinstall it.

A parked change is stored at `refs/fufu/open/<branch>`. For example, `git log -1 refs/fufu/open/parser-fix` inspects the saved work on `parser-fix`. In a clean working copy on the appropriate base, `git cherry-pick -n <sha>` can apply that saved commit's changes without committing them. Use the actual saved hash you inspected; this Git operation can conflict if the base has changed.

On return, fufu observes and reports the repository state as described above. Check status and follow any reported hold or repair instructions. The time spent away does not create snapshots retroactively. [Adopting fufu](../adopting.md) covers enabling it in an existing repository; the [plain-Git teammates guide](../guides/plain-git-teammates.md) has collaboration examples.
