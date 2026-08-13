<div align="center">

# fufu

**jj's workflow on git's repository**

*Automatic capture, effortless branching, whole-repo undo — on an ordinary<br>
git repository that stays legible to every tool and teammate.<br>
git remains the VCS. fufu is the pilot.*

</div>

---

fufu (`ff`) is built on the belief that jj got the workflow right and git got
the repository right — and that you can have both. At every instant, the
repository is a boring git repository: HEAD attached to a branch, ordinary
commits, `git status` legible. fufu automates the transitions between such
states — capture, movement, history rewriting, undo — and leaves the durable
graph entirely to git.

> **Early days.** fufu is young and under active construction. The design —
> what exists, what's coming, and why — lives in [DESIGN.md](DESIGN.md).
> Windows binaries are provided and CI-tested, but young.

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

## License

[MIT](LICENSE)
