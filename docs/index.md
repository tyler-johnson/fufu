# fufu

**git that flies itself.**

fufu (`ff`) is a version control interface for humans and agents: automatic snapshots, effortless branching, whole-repo undo. It is built on ordinary git, so your tools, your teammates, and your remotes all still work.

<video class="demo" autoplay loop muted playsinline>
  <source src="assets/demo.webm" type="video/webm">
  <img src="assets/demo.gif" alt="A terminal running ff: a glance at the branches, changes on main parked by a single switch, a commit, a fix folded into it, then sync and publish.">
</video>

At every instant, the repository is a boring git repository. fufu never creates a state plain git cannot represent; it only automates the transitions between such states. That one promise — [the invariant](concepts/invariant.md) — settles every design question in the tool.

The cost is up front: fufu is opinionated and will not meet you halfway.

- Branches rebase onto trunk — the main line of development — rather than merging it in.
- Unpublished commits stay malleable.
- There is no staging area.

Where that fits and where it does not is on [fufu vs git](comparisons/vs-git.md#the-honest-costs).

The daily loop is five verbs:

```console
$ ff start                        # begin new work — a fresh branch off trunk, nothing to name yet
$ ff commit -m "parser: handle unicode escapes"    # no add, no staging: the tree is the change
$ ff switch main                  # mid-edit is fine — this parks, that resumes
$ ff sync                         # line up with base and remote, replayed in memory, undoable
$ ff publish                      # the one thing fufu can't undo, so it's the one you type
```

And when anything goes wrong — including things done behind fufu's back with raw git — one [`ff undo`](reference/cli/undo.md) brings refs and working tree back together. It reaches as far back as the last capture — the snapshot fufu takes of the working tree before an action — and what gets captured is what the [hooks decide](comparisons/vs-git.md#the-honest-costs).

## Where to go

- [Install](install.md), then [`ff hook`](reference/cli/hook.md) — optional, recommended: it wires the shells and agent clients on this machine, and without it fufu captures only when you type an `ff` command.
- The [tutorial](tutorial.md): the whole loop once, with real transcripts.
- Already have a repository git made? [Adopting fufu](adopting.md) is [`ff init`](reference/cli/init.md) inside it.
- Deciding whether to switch from plain git: [fufu vs git](comparisons/vs-git.md) — what disappears, what stays, and what your aliases cannot do.
- Working alongside people who type git: [plain-git teammates](guides/plain-git-teammates.md) — what they see, what typing git yourself does, and what fufu can and cannot do about someone else's force-push.
- Coming from jj, or wondering why this exists at all: [fufu vs jj](comparisons/vs-jj.md) is the thesis.
- Pointing an agent at a repository: [why agents want fufu](agents/why.md) and [setup](agents/setup.md).
