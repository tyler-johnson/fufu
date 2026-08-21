<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

</div>

---

### Get a repository

`ff clone` is fufu's own, not a wrapper: it speaks the git protocol itself, checks out the worktree, and arms the repository on arrival — the gc guard written, and the operation log's floor laid, so `ff undo` has somewhere to land from your very first command. `ff init` does the same for a repository you are starting from nothing, and run inside one that already exists it means *turn fufu on here*.

```console
$ ff clone https://github.com/tyler-johnson/fufu.git
cloned into ./fufu — 145 commits on main
the net is on: ff undo has a floor to land on, and every verb takes one first

$ ff init                         # starting from nothing — or adopting a repo git made
initialized an empty repository on main
the net is on: ff undo has a floor to land on, and every verb takes one first
```

### Start new work

`ff start` begins new work, always on a fresh branch: it forks from trunk and hands you a clean tree, parking whatever you were in the middle of. Nothing to name up front — fufu mints a name and you claim it once the work has earned one.

```console
$ ff start                        # begin new work — a fresh tree off main, nothing to name yet
minted ff/pale-thicket (forked from main)
open change on ff/pale-thicket
undo: ff undo

$ ff describe -m "parser: handle unicode escapes"    # name the change while you work on it
pending description on ff/pale-thicket: parser: handle unicode escapes

$ ff commit                       # close it — no add, no staging, no -m: the tree was the commit
closed 9c873d66 on ff/pale-thicket: parser: handle unicode escapes (3 file(s))
undo: ff undo
```

### Resume work at any time

Forgot where you left that idea? Bare `ff` is the map: recent work on every branch, parked changes included. Switching away parks whatever you're in the middle of — no stash juggling — and switching back brings that tree in exactly as you left it.

```console
$ ff                              # where did I leave that idea?
@  otnkwlkl f14734ed   1m ago  ▸ [JIRA-1234/some-feature-im-adding]
│  api: surface rate-limit errors
●  vvurpzrm 1783983f   1h ago
│  api: retry with backoff
│ ●  —        c22fb0f8   3d ago  ▸ [ff/pale-thicket]  (+ parked change, 1 file)
│ │  parser: keep the lexer honest
│ ●  —        9c873d66   3d ago
├─╯  parser: handle unicode escapes
●  —        dacae370   5d ago  ▸ [main]
│  release: cut v0.4.1
~

$ ff switch ff/pale-thicket       # mid-edit is fine: this one parks, that one resumes
parked the open change on JIRA-1234/some-feature-im-adding (ebcc7dff)
switched to ff/pale-thicket
resumed the parked change (1 file(s))
undo: ff undo

$ ff describe -b unicode-cleanup    # it's real now — claim the name
claimed ff/pale-thicket as unicode-cleanup
undo: ff undo
```

### Maintenance on autopilot

The busywork between commits — fixup commits, autosquash dances, rebasing onto main — flies itself. `ff status` answers questions git makes you find out the hard way, `absorb` folds a review fix into the commit you name and restacks everything above it without moving a file on disk, and `sync` replays onto main only when it's safe. Sync takes in; `publish` sends. They are two verbs because everything sync does is undoable and a push is not.

```console
$ ff status                       # futures, not just facts: fufu already knows the rebase is safe
on unicode-cleanup · base moved — rebases cleanly (2 commits replayed)
@  nquyppwp 2e9cb947   2m ago
│  (no description)
│  M src/parser/escape.rs +1  -0  ++++++++++++++++++++
│  M src/parser/mod.rs    +1  -0  ++++++++++++++++++++
│    2 files              +2  -0
●  usqppxtv c22fb0f8   3d ago
│  parser: keep the lexer honest

$ ff absorb --into 9c873d66       # the fix belongs to that commit, not to a new one
absorbed into 410fdaba: parser: handle unicode escapes
restacked 1 commit(s) above it
undo: ff undo

$ ff sync                         # line up with base and remote — replays in memory, lands only if clean
fetching from origin
main moved ahead by 1 commit(s)
replayed 2 commit(s) onto main
updated the working tree (1 file(s))
not published yet — ff publish
undo: ff undo

$ ff publish                      # the one thing fufu can't undo, so it's the one you type
created origin/unicode-cleanup and set unicode-cleanup to track it
the push left the machine — ff undo cannot reach it
ff undo then ff publish rolls the shared copy back, under a lease
```

### Undo anything

fufu snapshots the repository around every operation — including the ones it didn't make. So when an agent (or you) runs something destructive with raw git, one `ff undo` brings back refs and working tree together.

```console
$ git reset --hard HEAD~2         # an overeager agent, 4pm on a Friday
HEAD is now at 03a98e3 api: clamp retry ceiling

$ ff undo                         # fufu snapshotted first — nothing was ever at risk
ff: absorbed changes made outside fufu:
  refs/heads/unicode-cleanup moved to 03a98e33 (reset: moving to HEAD~2)
undid (a change made outside fufu): absorbed 1 foreign ref change(s)
  now at rrntqnkwqrkr (published unicode-cleanup to origin/unicode-cleanup)
  refs/heads/unicode-cleanup → 1bc4a38c
  3 worktree file(s) restored
back: ff redo
```

## Install

Linux/macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.ps1 | iex
```

Homebrew:

```sh
brew install tyler-johnson/tap/fufu
```

## License

[MIT](LICENSE)
