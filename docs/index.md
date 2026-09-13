# fufu

**git that flies itself.**

Edit files, commit without staging, and switch tasks with unfinished work saved on the branch you leave. fufu (`ff`) is a version control interface for humans and agents, with automatic snapshots and undo of recorded local work. It uses an ordinary Git repository, so [your existing tools and teammates](concepts/two-regimes.md) can keep working with Git.

<div class="demo cast" data-cast="assets/demo.cast" data-loop>
  <noscript><img src="assets/demo.gif" alt="A terminal running ff: a glance at the branches, changes on main parked by a single switch, a commit, a fix folded into it, then pull and push."></noscript>
</div>

The everyday workflow:

- **Commit your changes.** Your working copy is the open change. [`ff commit`](reference/cli/commit.md) records eligible edits in branch history without a staging step.
- **Switch tasks mid-edit.** [`ff switch`](reference/cli/switch.md) parks unfinished work with the branch you leave and resumes the destination branch's work. Return to pick up where you stopped.
- **Fix earlier commits.** [`ff absorb`](reference/cli/absorb.md) adds edits to an existing commit and replays the commits above it. If a replay conflicts, fufu keeps a [held rewrite](concepts/held-rewrites.md) to resolve; you can work on another branch meanwhile.
- **Recover recorded work.** [`ff undo`](reference/cli/undo.md) restores local refs and files from the current worktree's operation log. It can recover a bad edit or hard reset when the earlier state was captured and retained. [Snapshot coverage](concepts/snapshots-and-undo.md#coverage-and-limits) excludes uncaptured edits, ignored untracked files, oversized working-copy content, and remote effects.
- **First-class agent support.** A shipped skill and built-in nudging. Installed and active hooks attempt snapshots before the tool calls they cover, giving an agent recovery points for its edits.
- **It's still git.** Real commits, real branches, an ordinary repository that every tool and teammate reads as one. Worktrees, remotes, hooks, and the rest of git are all still there.

## Start here

[Install fufu](install.md), then follow the [tutorial](tutorial.md). It takes you from a disposable local repository through commits, switching, rewriting, pulling, pushing, and undo, with exact edits and real output.

## Where to go

- Already have a repository? [Adopting fufu](adopting.md) starts with [`ff init`](reference/cli/init.md) inside it.
- Need to recover work? [Recovery](guides/recovery.md) starts with the available undo steps.
- Deciding whether to switch from plain git: [fufu vs git](comparisons/vs-git.md) — what disappears, what stays, and what your aliases cannot do.
- Working alongside people who type git: [plain-git teammates](guides/plain-git-teammates.md) — what they see, what typing git yourself does, and what fufu can and cannot do about someone else's force-push.
- Coming from jj, or wondering why this exists at all: [fufu vs jj](comparisons/vs-jj.md) is the thesis.
- Pointing an agent at a repository: [why agents want fufu](agents/why.md) and [setup](agents/setup.md).
