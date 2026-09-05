# FAQ

Short answers, with a link to the page that owns each full story.

## Is my repository still a normal git repository?

Yes, always. At every instant it is a boring git repository: HEAD attached, ordinary commits, `git status` reading the way it always reads, and nothing a teammate, GUI, or CI job can tell apart from careful plain git.

fufu never creates a state plain git cannot represent. It only automates the moves between states git already has. This is [the invariant](concepts/invariant.md), and every other design question in the tool is settled by asking what preserves it.

## Can I stop using fufu? Can I use it on one machine and not another?

Both, freely. Everything fufu writes is ordinary git — snapshots are refs outside the visible graph, parked changes are labeled stash entries — so deleting fufu loses convenience and never data. The stash dance comes back and the manual rebase comes back, but no commit, branch, or file state is lost.

Leaving does not have to be total or permanent either. A machine without fufu, a weekend of raw git, or a GUI session are all absorbed when you return: the first fufu operation back compares what it remembered against what it finds, and says out loud anything that changed.

[Adopting fufu](adopting.md) covers trying it and leaving; [the two regimes](concepts/two-regimes.md) covers coming back.

## What happens when a teammate force-pushes or rewrites history I've built on?

Nothing is lost, and nothing is sent by accident. Every [`ff publish`](reference/cli/publish.md) carries a lease, so if the shared copy moved since you last saw it, the push is refused and your commits stay put.

[`ff sync`](reference/cli/sync.md) then reconciles by whose divergence it is. Divergence the fetch just revealed is somebody else's work, so their commits are taken in and yours replay on top. A commit of yours the rewrite already contains replays empty and is dropped, with sync saying which.

The whole sync is one operation that one [`ff undo`](reference/cli/undo.md) takes back. The walkthrough with real output is in [recovery](guides/recovery.md#someone-force-pushed-over-my-branch); the divergence rules live in [the push boundary](concepts/push-boundary.md).

## Does fufu work with GitHub, GitLab, and other forges?

Yes, with any forge that serves the git protocol. There is nothing forge-specific to support: a remote is a git remote, a push is a git push under force-with-lease, and nothing server-side knows fufu exists.

Your existing credential helpers, `url.insteadOf` rewrites, and proxies are honored, so a repository that already authenticates keeps working with nothing new to configure.

Gerrit's review flow is the one caveat. fufu has no verb for pushing to a magic ref like `refs/for/main`, so that flow stays [`ff git push`](reference/cli/git.md), and it is untested. See [what stays git](comparisons/vs-git.md#what-stays-git) and [configuration](reference/config.md#what-fufu-reads-from-gits-config).

## Does fufu work with git LFS?

Not yet, honestly. fufu's native substrate does not implement the LFS contract, and no part of the tool is tested against it. The [substrate](internals/substrate.md) page lists filters and LFS in the long tail of git ecosystem contracts that follows as the substrate matures.

If your repository depends on LFS today, treat fufu as unsupported there rather than hoping.

Relatedly, snapshots skip new files larger than `fufu.maxFileSize` (50 MiB by default) and say so, which bounds what capture will carry in large-asset repositories. See [configuration](reference/config.md#maxfilesize).

## Does fufu work with submodules?

fufu has no verbs for submodules. They stay git's, reached through `ff git submodule …`, which snapshots first and then runs git verbatim. Those commands pass untouched even under strict mode, because fufu only refuses git words it has a verb for.

Beyond that passthrough, submodule repositories are untested territory. The [substrate](internals/substrate.md) page places them in the same not-yet long tail as LFS. See [what stays git](comparisons/vs-git.md#what-stays-git).

## Where does fufu keep its state, and how big does it get? What does `ff trim` do?

Everything fufu writes lives in two places inside the repository. Refs under `refs/fufu/` hold the operation log, snapshot pointers, and parked-entry and published-tip records. Plain files under `<common-dir>/fufu/` hold caches and branch metadata. None of it is pushed, and all of it is a cache over git rather than an authority.

Size is bounded by retention. `ff trim` drops operations past the `fufu.keep` window (90 days by default), rides an ordinary command at most once per `fufu.autoTrim` (daily by default), and nudges git's own gc when it dropped something. The last trim is itself recoverable from a trash ref.

[Architecture](internals/architecture.md#where-fufus-state-lives) maps the layout; [`ff trim`](reference/cli/trim.md) and [configuration](reference/config.md) cover the knobs.

## Why is there no staging area? I liked the staging area.

Because the working copy is the change. There is no object to assemble before committing, and [`ff commit`](reference/cli/commit.md) closes the tree into a commit in one step.

What the index gave you survives as an argument instead of a state. `ff commit <paths>` closes a slice and leaves the rest open — selection made once at the moment of the close, with nothing to maintain between commits.

The index still exists underneath, and hook-runners still see it staged correctly. You just never curate it by hand. [Changes](concepts/changes.md) is the model; [fufu vs git](comparisons/vs-git.md#what-disappears) is the argument.

## Can I commit some hunks of a file and leave the rest?

Not through fufu's own verbs. A partial commit selects by path — a file, or a directory — and there is no hunk-level selection.

When one file holds two changes, the escape hatch is `ff git commit -p`, which snapshots first and then lets git build that commit interactively from the worktree. It is exactly the `-p` you know.

Two things bound that hatch:

- Under `fufu.gitPolicy strict` the command is refused along with every other `git commit`, so a hunk commit needs the policy set to `coach` or `observe` instead. See [what strict refuses](#what-does-strict-mode-refuse).
- `ff git add -p` followed by `ff commit` does not work. `ff commit` closes the worktree rather than the index, so a hand-staged selection would be overwritten instead of honored.

Hunk-level selection through a fufu verb is a genuine capability gap today, not a workflow the docs are steering you around. [Changes](concepts/changes.md) is the model partial commits do follow.

## Why can't `ff undo` take back a push?

Because a push is the one act that leaves the machine. Other clones can fetch it, CI runs on it, webhooks fire, and no operation log on your machine reaches any of that.

So undo is honest about its reach, and rollback is a different, still-guarded act. `ff undo` moves your local branch back, and the next `ff publish` rolls the shared copy back to match, under a lease that stops if somebody pushed in the meantime.

Rollback is not erasure — commits that reached the world stay reached — but the shared copy is yours to move. [The push boundary](concepts/push-boundary.md) is the full story.

## What does strict mode refuse?

`fufu.gitPolicy strict` refuses exactly the git writes fufu has a verb for — `git commit`, `commit -p` included, `git stash push`, `git reset`, and their kin — and names the fufu verb to run instead. It never silently runs something in the refused command's place.

Reads pass untouched at every level. So do writes with no fufu answer, such as `apply`, `am`, `bisect`, and `submodule`. Ambiguous compound shell strings fail open rather than guessing.

The capture already happened before the command ran either way, so the policy is a nudge with teeth rather than the safety net itself. See [plain-git teammates](guides/plain-git-teammates.md#the-alias-and-gitpolicy) and [why agents](agents/why.md).

## Why was `ff status` refused in the shell?

Because the `ff` tool was up for that session, and `fufu.toolPolicy` is `strict` by default.

Claude Code's plugin registers [`ff mcp`](reference/cli/mcp.md) as a tool beside the capture hook. While that server is serving, an `ff` run through the shell tool is refused with a reason naming the tool and the exact `{"args": […]}` to call it with — the same words, no quoting, structured results.

The seven shell-only verbs always pass: `ff git`, [`ff update`](reference/cli/update.md), [`ff watch`](reference/cli/watch.md), [`ff hook`](reference/cli/hook.md), [`ff unhook`](reference/cli/unhook.md), `ff mcp`, and [`ff extension`](reference/cli/extension.md). Nothing is refused when no server is up.

[`ff config toolPolicy coach`](reference/cli/config.md) turns the refusal into a one-line nudge, and `observe` turns it off. See [agent setup](agents/setup.md#serve-the-verbs-as-a-tool).

## How far back can undo reach? What about before I ran `ff init`?

Undo reaches back to the floor: the operation log's first entry, taken from observed state at the moment fufu was armed.

Everything before fufu's arrival is git's history rather than fufu's timeline. It is still reachable with git's own tools, but it is not a place `ff undo` can land, and nothing becomes undoable retroactively.

The same bound applies day to day. Work done around fufu is protected only as far back as the last capture, so a raw `git restore <file>` can discard edits fufu never saw. [Snapshots and undo](concepts/snapshots-and-undo.md#the-floor) covers the floor; [recovery](guides/recovery.md#what-undo-cannot-reach) shows it in practice.

## Does fufu run my git hooks?

Yes — git's four commit-time hooks, from every verb where git's equivalent operation would run them. fufu runs them itself, resolving through `core.hooksPath` and aborting the verb on a non-zero exit, exactly as git does.

Hook-runners like lefthook, lint-staged, and husky work too. fufu writes the index to the tree it is about to commit before the first hook fires, so a runner that asks git what is staged sees the right answer.

One rule decides the table: the tree hook runs where worktree content becomes commit content, and the message hooks run where a message is authored for a commit.

| verb | pre-commit | prepare-commit-msg | commit-msg | post-commit |
|---|---|---|---|---|
| `ff commit` | yes | yes | yes | yes |
| [`ff absorb`](reference/cli/absorb.md) | yes | no — it inherits the target's message untouched | no | no |
| [`ff done`](reference/cli/done.md) (edit session) | yes | only when the session carries a new description | same condition | no |
| `ff done` (resolution landing) | yes | no | no | no |
| [`ff describe <rev>`](reference/cli/describe.md) | no — no tree moves | yes | yes | no |
| `ff describe` (open change) | no | no — a pending description is not a commit; the hooks fire when it closes | no | no |
| [`ff lift`](reference/cli/lift.md) | no — no worktree content enters a commit | no | no | no |
| [`ff restack`](reference/cli/restack.md), `ff sync` | no — `git rebase` runs none either | no | no | no |

`post-commit` stays on `ff commit` alone, because git fires it from `git commit` and not from `rebase`, and absorb, done and describe are rebases.

`--no-verify` skips `pre-commit` and `commit-msg` on every verb that can be declined. git documents `prepare-commit-msg` as not skipped by it, and fufu follows.

`pre-merge-commit` does not apply, since fufu never writes a merge commit, and `applypatch-*` belong to `git am`. [Substrate](internals/substrate.md#behavioral-compatibility) has the details, including the one deliberate divergence around formatter fixes.

## Do I need git installed?

Mostly no, eventually not at all. The daily surface — status, commit, switch, sync's fetch, undo, log, restore, and the rest — runs in-process with no git on the machine.

Four things still want git on PATH: the push (until gix can send a pack), credential helpers and ssh where a remote needs them, trim's best-effort `gc --auto` (skipped silently without it), and the `ff git` escape hatch. [Substrate](internals/substrate.md#the-git-free-destination) tracks the line as it moves.

## What's the name about?

jj is short for Jujutsu, the martial art of redirecting force instead of opposing it, and fufu answers from the same dojo. "fu" is the syllable hacker culture borrowed for tool mastery — git-fu, shell-fu — doubled.

In Japanese, fūfu (夫婦) is a married couple: two who operate as one, which is the architecture. fufu and git, one household. It is also a West African dish of starch pounded until smooth, which is roughly what fufu does to git.

The binary is `ff`, the left hand's mirror of `jj`. The [design document](internals/design.md) tells it in the founders' words.
