# Install

## 1. Get the binary

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

Check the result:

```console
$ ff version
fufu 0.10.0 (0a47458 2026-09-01)
https://github.com/tyler-johnson/fufu
```

## 2. Wire it in

Optional, and recommended.

```sh
ff hook
```

fufu captures only when something invokes it, so without hooks only an `ff` command captures. With them a snapshot lands before every agent tool call, every git command you type, and every shell prompt.

Bare `ff hook` reports the shells and agent clients it found and asks; `--all` takes everything detected, `-l` reports and stops. Claude Code and Codex get [fufu's skill](agents/setup.md) with the wiring. Once per machine, not per repository.

## What you installed

fufu ships as a single `ff` binary. Most of what it does is native, but today it still reaches your git installation for a few operations — the push, credential helpers, hooks — so keep git installed. `ff doctor` reports what fufu found and whether the repository it is standing in is armed.

`ff update` names the command that updates this copy of fufu — the install script, `brew upgrade fufu`, `cargo install`, or whatever else placed the binary — and offers to run it. Nothing updates itself unasked.

Next: the [tutorial](tutorial.md), or [adopting fufu](adopting.md) if you already have a repository.
