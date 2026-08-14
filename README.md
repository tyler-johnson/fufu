<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

</div>

---

### Start new work

Point `ff start` at anything — even a ref you only know from the remote — and go. There's no branch to name up front: fufu mints one and you claim it once the work has earned a name.

```console
$ ff start main@origin            # new work, straight off the remote — nothing to name yet
minted ff/quiet-lake (forked from main@origin)
open change on ff/quiet-lake
undo: ff undo

$ ff describe -m "parser: handle unicode escapes"    # name the change while you work on it
pending description on ff/quiet-lake: parser: handle unicode escapes

$ ff commit                       # close it — no add, no staging, no -m: the tree was the commit
closed 2c9ea49 on ff/quiet-lake: parser: handle unicode escapes (3 file(s))
undo: ff undo
```

### Resume work at any time

Forgot where you left that idea? Bare `ff` is the map: recent work on every branch, parked changes included. Switching brings the tree back exactly as you left it.

```console
$ ff                              # days later: where did I leave that idea?
@  no changes
│  (no description)
●  rsqmkwtv 1f4c2d7      5m  JIRA-1234/some-feature-im-adding
│  api: retry with backoff
│ ●  wpxvqnkz 2c9ea49    3d  ff/quiet-lake
├─╯  parser: handle unicode escapes  (+ parked change, 1 file)
●  tkzvmwqx 5b7a90e      2h  main
│  release: cut v0.4.1

$ ff switch ff/quiet-lake         # right where you left it
switched to ff/quiet-lake
resumed the parked change (1 file(s))
undo: ff undo

$ ff describe -b unicode-cleanup -m "really great idea I had"    # it's real now — claim the name
renamed ff/quiet-lake to unicode-cleanup
pending description on unicode-cleanup: really great idea I had
undo: ff undo
```

### Maintenance on autopilot

The busywork between commits — stashing, fixup commits, rebasing onto main — flies itself. Switching parks mid-edit work, `absorb` files review fixes into the commits they belong to, and `sync` lands you on main only when it's safe.

```console
$ ff switch JIRA-1234/some-feature-im-adding    # mid-edit? parked, not stashed
parked the open change on ff/quiet-lake (84db9582)
switched to JIRA-1234/some-feature-im-adding
resumed the parked change (2 file(s))
undo: ff undo

$ ff status                       # futures, not just facts: fufu already knows the rebase is safe
on JIRA-1234/some-feature-im-adding · behind 4 of origin/main
unstaged:
  M  src/api/retry.rs
  M  src/api/error.rs
main moved — this branch rebases cleanly

$ ff absorb                       # review fixes fold into the commits they belong to
absorbed 3 hunks into 2 commits:
  1f4c2d7 api: retry with backoff
  b93e001 api: surface rate-limit errors
descendants rebased in memory
undo: ff undo

$ ff sync                         # catch up with main — rebases in memory, lands only if clean
fetched origin: main 9f3d2c1 → 5b7a90e (4 new commits)
JIRA-1234/some-feature-im-adding rebases cleanly — landed (3 commits replayed)
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

## License

[MIT](LICENSE)
