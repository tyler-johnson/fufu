# fufu

**git that flies itself.**

fufu (`ff`) is a version control interface for humans and agents: automatic snapshots, effortless branching, whole-repo undo. It is built on ordinary git, so your tools, your teammates, and your remotes all still work.

At every instant, the repository is a boring git repository. fufu never creates a state plain git cannot represent; it only automates the transitions between such states. That one promise — [the invariant](concepts/invariant.md) — settles every design question in the tool.

The daily loop is five verbs:

```console
$ ff start                        # begin new work — a fresh branch off trunk, nothing to name yet
$ ff commit -m "parser: handle unicode escapes"    # no add, no staging: the tree is the change
$ ff switch main                  # mid-edit is fine — this parks, that resumes
$ ff sync                         # line up with base and remote, replayed in memory, undoable
$ ff publish                      # the one thing fufu can't undo, so it's the one you type
```

And when anything goes wrong — including things done behind fufu's back with raw git — one `ff undo` brings refs and working tree back together.

## Where to go

- [Install](install.md), then `ff hook` — optional, recommended: it wires the shells and agent clients on this machine, and without it fufu captures only when you type an `ff` command.
- The [tutorial](tutorial.md): the whole loop once, with real transcripts.
- Already have a repository git made? [Adopting fufu](adopting.md) is `ff init` inside it.
- Deciding whether to switch from plain git: [fufu vs git](comparisons/vs-git.md) — what disappears, what stays, and what your aliases cannot do.
- Coming from jj, or wondering why this exists at all: [fufu vs jj](comparisons/vs-jj.md) is the thesis.
- Pointing an agent at a repository: [why agents want fufu](agents/why.md) and [setup](agents/setup.md).
