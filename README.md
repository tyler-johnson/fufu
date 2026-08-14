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
$ ff                              # days later: where did I leave that idea?
@  qpmvkzrt 7a3f1c8      2m  JIRA-1234/some-feature-im-adding
│  api: surface rate-limit errors
●  rsqmkwtv 1f4c2d7     26m
│  api: retry with backoff
│ ●  wpxvqnkz 2c9ea49    3d  ff/quiet-lake
├─╯  parser: handle unicode escapes  (+ parked change, 1 file)
●  tkzvmwqx 5b7a90e      2h  main
│  release: cut v0.4.1

$ ff switch ff/quiet-lake         # mid-edit is fine: this one parks, that one resumes
parked the open change on JIRA-1234/some-feature-im-adding (84db9582)
switched to ff/quiet-lake
resumed the parked change (1 file(s))
undo: ff undo

$ ff describe -b unicode-cleanup -m "really great idea I had"    # it's real now — claim the name
renamed ff/quiet-lake to unicode-cleanup
pending description on unicode-cleanup: really great idea I had
undo: ff undo
```

### Maintenance on autopilot

The busywork between commits — fixup commits, autosquash dances, rebasing onto main — flies itself. `ff status` answers questions git makes you find out the hard way, `absorb` files review fixes into the commits they belong to, and `sync` lands you on main only when it's safe.

```console
$ ff status                       # futures, not just facts: fufu already knows the rebase is safe
on unicode-cleanup · behind 4 of origin/main
unstaged:
  M  src/parser/escape.rs
  M  src/parser/lexer.rs
main moved — this branch rebases cleanly

$ ff absorb                       # review fixes fold into the commits they belong to
absorbed 3 hunks into 2 commits:
  2c9ea49 parser: handle unicode escapes
  d81b3f6 parser: fold surrogate pairs
descendants rebased in memory
undo: ff undo

$ ff sync                         # catch up with main — rebases in memory, lands only if clean
fetched origin: main is at e1c47a2 (4 new commits)
unicode-cleanup rebases cleanly — landed (2 commits replayed)
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

Installed binaries check for a new release in the background about once a day and quietly update themselves. `ff config updateCheck false` turns that off entirely; `ff config autoUpdate false` keeps the check but prints a one-line notice instead of installing.

`ff doctor` verifies the whole net in one pass — chains, snapshot identity, reflogs, the gc guard, hook and alias wiring, update state — and exits 1 on findings; `--fix` repairs the one thing it's allowed to, the gc config.

## License

[MIT](LICENSE)
