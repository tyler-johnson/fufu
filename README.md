<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

</div>

---

```console
$ ff describe -m "parser: handle unicode"    # name the change while you work on it
pending description on main: parser: handle unicode

$ ff new                     # done — no add, no staging: the working tree was the commit
closed 2c9ea49 on main: parser: handle unicode
open change on main

$ ff switch hotfix           # mid-edit? switching parks the change, no stash juggling
parked the open change on main (84db9582)
switched to hotfix

$ ff switch main             # and coming back resumes it
switched to main
resumed the parked change (1 file(s))
undo: ff undo
```

fufu (`ff`) is a version control interface: it drives git, it doesn't replace it. The workflow — nothing ever unsaved, branch at will, undo everything — is the one jj proved, but where jj makes git a backend for its own store, fufu inverts the architecture: at every instant the repository is a boring git repository — HEAD attached to a branch, ordinary commits, `git status` legible. fufu automates the transitions between such states — capture, movement, undo — and leaves the durable graph entirely to git.

> **Early days.** fufu is young and under active construction. The design — what exists, what's coming, and why — lives in [DESIGN.md](DESIGN.md). Windows binaries are provided and CI-tested, but young.

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

Then wire the capture hooks:

```sh
ff hook shell install          # alias git='ff git' in your shell rc
ff hook agent install claude   # capture around Claude Code's tool actions
```

Settings live in plain git config under `fufu.*` — `ff config` lists every one with its value and meaning; `ff config keep 30d` sets one.

## License

[MIT](LICENSE)
