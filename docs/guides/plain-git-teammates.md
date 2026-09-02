# Plain-git teammates

Nobody else has to know you run fufu. [The invariant](../concepts/invariant.md) is the reason: at every instant the repository is a boring git repository, so every person and every tool that speaks git keeps working without noticing you. This page is what that looks like in practice — what your teammates see, what happens when you type git yourself, and what fufu does and does not ask of the people around you. Every transcript is real `ff` output.

The console blocks below share one scene: a fresh clone, a branch named `parser` with a commit on it, all made through fufu.

```console
$ ff start -b parser
minted parser (forked from main)
open change on parser
undo: ff undo

$ ff commit -m "lexer: skeleton"
closed 5a56e702 on parser: lexer: skeleton (1 file(s))
undo: ff undo
```

## What everyone else sees

A teammate who pulls your branch sees ordinary commits on an ordinary branch. A GUI sees branches where branches should be, HEAD attached, `git status` reading the way it always reads. CI checks out the commit it was asked to build. Nothing fufu stores reaches a remote: snapshots live in refs under `refs/fufu/`, beside the visible history rather than in it, and no push carries them.

The one piece of fufu state a plain-git tool can even encounter is a parked change, and it is deliberately the most boring thing it could be: an ordinary stash entry, labeled with its branch, in the same stash panel every GUI already has. Park something by switching away, then look at it with plain git:

```console
$ ff switch main
parked the open change on parser (a8e1cce2)
switched to main
undo: ff undo

$ git stash list
stash@{0}: On parser: fufu: wip on parser
```

A teammate who opens this repository does not need fufu explained to them, and the entry is next to the button that would restore it. Switch back with `ff switch parser` and the change resumes with its branch; the entry leaves the panel. [Branches](../concepts/branches.md) covers parking itself.

## Your own git tools: reads and writes

You are allowed to keep your git habits. The rule that sorts every case is in [the two regimes](../concepts/two-regimes.md): operations through fufu get fufu's guarantees, operations around it get git's exact documented behavior, absorbed afterward.

Reads are always fine, with nothing to absorb. `git log`, `git blame`, `git diff`, `gitk`, your IDE's history panel — use them freely and forever. fufu adds no state a reader has to understand.

Writes are fine too, and this is the part worth seeing once. Commit from an IDE, or with raw git in another terminal:

```console
$ git commit -am "docs: say what this is"
[parser 3dbae0c] docs: say what this is
 1 file changed, 1 insertion(+)
```

fufu was not watching and did not interfere; git did exactly what git does. At your next fufu operation, the difference between what fufu remembered and what the repository now says is noticed, folded into the operation log as a foreign operation, and said out loud:

```console
$ ff status
on parser · nothing to sync
@  no changes
│  (no description)
●  —        3dbae0c0   0s ago
│  docs: say what this is
changes made outside fufu (absorbed; ff undo can roll them back):
  refs/heads/parser moved to 3dbae0c0
```

The notice stays pinned in `ff status` while the log's tip is foreign, so motion fufu did not perform is never silently blended into motion it did. And because the commit is in the operation log now, `ff undo` can take it back like anything fufu did itself — here the commit was wanted, so it simply stays.

## `ff git`: the escape hatch that keeps undo working

The gap in the lazy story is the working tree. A foreign ref move is always recoverable from the log, but a raw command that rewrites files can destroy tree state that existed only since the last capture. `ff git <args…>` closes that gap: it snapshots first, then runs git verbatim — no flags reinterpreted, no behavior second-guessed. Whatever git has that fufu lacks a verb for, this is how you reach it without stepping off the safety net.

The demonstration is the most destructive habit in git's repertoire:

```console
$ ff git reset --hard HEAD~1
ff: tip: that's ff undo
HEAD is now at 5a56e70 lexer: skeleton

$ ff undo
ff: absorbed changes made outside fufu:
  refs/heads/parser moved to 5a56e702 (reset: moving to HEAD~1)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at xlrtxownxrrw (absorbed 1 foreign ref change(s))
  refs/heads/parser → 3dbae0c0
  1 worktree file(s) restored
back: ff redo
```

The reset really ran, and one `ff undo` brought back refs and worktree together. The `tip:` line is fufu coaching, covered next; the reset itself was never blocked. See [the git passthrough reference](../reference/cli/git.md) for the details.

## The alias and gitPolicy

[`ff hook bash`](../reference/hooks/bash.md) (or `zsh`, `fish`, `powershell`) installs `alias git='ff git'` in your shell, so typed git lands on the fufu surface by spelling you already have as muscle memory. The boundary is execution path: aliased git is captured first and absorbed as fufu's own, while anything that resolves git on PATH — a GUI, a script, a teammate — stays outside and is absorbed lazily as above.

What fufu says when git is reached for through it is the `fufu.gitPolicy` setting, with three levels:

- **`observe`** records and stays quiet.
- **`coach`**, the default, names the fufu verb once per git word — the `ff: tip: that's ff undo` line above — and then runs the command anyway.
- **`strict`** refuses a git write that has a fufu verb, and names what to run instead.

```console
$ ff config gitPolicy strict
gitPolicy = strict (this repo)

$ ff git commit -m wip
ff: fufu.gitPolicy is strict, and fufu has a verb for git commit: ff commit — the working tree is the change, and ff commit closes it onto the log
  try:
    ff commit
    ff config gitPolicy coach
```

Nothing is ever silently run in the refused command's place, and reads pass untouched at every level:

```console
$ ff git log --oneline -n 2
3dbae0c docs: say what this is
5a56e70 lexer: skeleton
```

Strict is at its best keeping an agent on the fufu surface — [agents setup](../agents/setup.md) wires the same policy through tool hooks — but it works the same on your own fingers while the reflexes retrain.

## A weekend away

You can leave entirely: a laptop without fufu installed, a week living in a GUI, a colleague driving your checkout with raw git. Nothing accumulates while you are gone, because there is no fufu-shaped consistency for plain git to violate — fufu's records are a cache over git, and the repository wins every disagreement.

Coming back is reconciliation. At your first fufu operation, everything that happened in the meantime is observed, absorbed into the timeline as foreign operations, and reported — a branch that moved, a parked entry dropped by hand, a commit rewritten behind fufu's back, each said out loud rather than silently forgotten. Then the guarantees resume. [The two regimes](../concepts/two-regimes.md) walks the weekend in full, and [the invariant](../concepts/invariant.md) explains why the return can never find anything broken.

## What fufu asks of the branch, and of the repo

Of the repository and the people in it, fufu asks nothing. No server-side setup, no hooks your teammates must install, no workflow the rest of the team must adopt, no trace in the pushed history that fufu was involved. How work lands on the shared branch — merge commit, squash, rebase — remains the team's and the forge's business. The same fact is a limit: fufu cannot stop a teammate's raw-git force-push over a shared branch, because nothing of fufu runs on their machine or on the server. Prevention is a branch protection rule on the forge; what fufu holds is the recovery half, [when someone force-pushed over your branch](recovery.md#someone-force-pushed-over-my-branch).

Of your own unpublished branches, fufu is opinionated: they rebase onto main rather than merging it in, unpublished commits stay malleable, and updating the remote copy of your branch after a rewrite is a leased force-push — sent only if the shared copy still stands where you last saw it. Those opinions are confined to work only you can see, and they stop at [the push boundary](../concepts/push-boundary.md): published history is append-only.

A teammate looking at your branch sees the result of that discipline — a clean stack of commits atop current main — and nothing of the machinery. Which is the invariant doing its job.
