# fufu

**git that flies itself.**

fufu (`ff`) is an opinionated version control interface for humans and agents: automatic snapshots, effortless branching, whole-repo undo. It is [built on ordinary git](concepts/invariant.md), so your tools, your teammates, and your remotes all still work.

<div class="demo cast" data-cast="assets/demo.cast" data-loop>
  <noscript><img src="assets/demo.gif" alt="A terminal running ff: a glance at the branches, changes on main parked by a single switch, a commit, a fix folded into it, then sync and publish."></noscript>
</div>

fufu is version control done the right way:

- **Commits, all the way down.** Your working copy is an open commit. There is nothing to stage, nothing to stash, and nothing to track. When you are done making changes, close the current commit and start on the next.
- **Move HEAD, without friction.** The working copy stays with the branch. Switch, and the open commit goes with it. When you return, everything is right where it should be.
- **Nothing stops the world.** Step back onto any commit and edit it; everything above it rebases on its own. A conflict is just another edit. Walk away from one mid-rebase: switch branches, come back whenever you like.
- **Undo anything.** Every operation is recorded, which makes everything undoable. This is git's reflog for the whole repo — mid-commit file edits, a bad merge on top of changes, a hard git reset.
- **First-class agent support.** Native MCP, leveraged skills, and built-in nudging. A snapshot lands before every agent tool call, letting a sloppy agent reverse its bad decisions.
- **It's still git.** Real commits, real branches, an ordinary repository that every tool and teammate reads as one. Worktrees, remotes, hooks, and the rest of git are all still there.

## Where to go

- [Install](install.md), then [`ff hook`](reference/cli/hook.md) — optional, recommended: it wires the shells and agent clients on this machine, and without it fufu captures only when you type an `ff` command.
- The [tutorial](tutorial.md): the whole loop once, with real transcripts.
- Already have a repository git made? [Adopting fufu](adopting.md) is [`ff init`](reference/cli/init.md) inside it.
- Deciding whether to switch from plain git: [fufu vs git](comparisons/vs-git.md) — what disappears, what stays, and what your aliases cannot do.
- Working alongside people who type git: [plain-git teammates](guides/plain-git-teammates.md) — what they see, what typing git yourself does, and what fufu can and cannot do about someone else's force-push.
- Coming from jj, or wondering why this exists at all: [fufu vs jj](comparisons/vs-jj.md) is the thesis.
- Pointing an agent at a repository: [why agents want fufu](agents/why.md) and [setup](agents/setup.md).
