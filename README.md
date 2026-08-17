<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

</div>

---

### Start new work

`ff start` begins new work, always on a fresh branch: it fetches main, forks from the fetched tip, and hands you a clean tree. Nothing to name up front — fufu mints a name and you claim it once the work has earned one.

```console
$ ff start                        # begin new work — a fresh tree off main, nothing to name yet
fetched origin: main is at 5b7a90e
minted ff/quiet-lake (forked from main)
open change on ff/quiet-lake
undo: ff undo

$ ff describe -m "parser: handle unicode escapes"    # name the change while you work on it
pending description on ff/quiet-lake: parser: handle unicode escapes

$ ff commit                       # close it — no add, no staging, no -m: the tree was the commit
closed 2c9ea49 on ff/quiet-lake: parser: handle unicode escapes (3 file(s))
undo: ff undo
```

### Resume work at any time

Forgot where you left that idea? Bare `ff` is the map: recent work on every branch, parked changes included. Switching away parks whatever you're in the middle of — no stash juggling — and switching back brings that tree in exactly as you left it.

```console
$ ff                              # where did I leave that idea?
@  xvvvrvlz d5d43cb   1m ago  JIRA-1234/some-feature-im-adding
│  api: surface rate-limit errors
●           c35c701   1h ago
│  api: retry with backoff
│ ●           6b78881   3d ago  ff/quiet-lake  (+ parked change, 1 file)
├─╯  parser: handle unicode escapes
●           aa86694   2h ago  main
   release: cut v0.4.1

$ ff switch ff/quiet-lake         # mid-edit is fine: this one parks, that one resumes
parked the open change on JIRA-1234/some-feature-im-adding (84db9582)
switched to ff/quiet-lake
resumed the parked change (1 file(s))
undo: ff undo

$ ff describe -b unicode-cleanup    # it's real now — claim the name
claimed ff/quiet-lake as unicode-cleanup
undo: ff undo
```

### Maintenance on autopilot

The busywork between commits — fixup commits, autosquash dances, rebasing onto main — flies itself. `ff status` answers questions git makes you find out the hard way, `absorb` folds a review fix into the commit you name and restacks everything above it without moving a file on disk, and `sync` lands you on main only when it's safe.

```console
$ ff status                       # futures, not just facts: fufu already knows the rebase is safe
on unicode-cleanup · base moved — rebases cleanly (2 commits replayed) · 2 to push
@  qzrtmvwk a3c7e91   2m ago
│  (no description)
│  M src/parser/escape.rs  +5  -2  ++++--
│  M src/parser/lexer.rs  +18  -4  ++++++++++++++++----
│    2 files              +23  -6
●           2c9ea49   3d ago
│  parser: handle unicode escapes

$ ff absorb --into 2c9ea49        # the fix belongs to that commit, not to a new one
absorbed into 8f1d3ba: parser: handle unicode escapes
restacked 1 commit(s) above it
undo: ff undo

$ ff sync                         # line up with base and remote — rebases in memory, lands only if clean
base moved to e1c47a2 (4 new commits)
rebases cleanly — landed (2 commits replayed)
pushed 2 commits to the remote
undo: ff undo
```

### Undo anything

fufu snapshots the repository around every operation — including the ones it didn't make. So when an agent (or you) runs something destructive with raw git, one `ff undo` brings back refs and working tree together.

```console
$ git reset --hard HEAD~3         # an overeager agent, 4pm on a Friday
HEAD is now at 9f3d2c1 api: retry with backoff

$ ff undo                         # fufu snapshotted first — nothing was ever at risk
undid 84f0c2d1 (a change made outside fufu): reset: moving to HEAD~3
  JIRA-1234/some-feature-im-adding → 1f4c2d7
  5 worktree file(s) restored
redo: ff undo
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
