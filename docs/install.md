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

### Pin and verify

Both installers take a version: set `FF_VERSION=vX.Y.Z` (`$env:FF_VERSION` in PowerShell) and the script installs exactly that release instead of the latest. Either way the script verifies the download's sha256 against the `checksums.txt` published with the release before anything lands on your PATH.

To skip the scripts entirely, every release publishes versioned archives — `ff_<version>_<os>_<arch>.tar.gz`, `.zip` on Windows — beside their `checksums.txt` on the [releases page](https://github.com/tyler-johnson/fufu/releases). Download the archive and `checksums.txt` into one directory, verify, and put `ff` on your PATH:

```console
$ sha256sum -c --ignore-missing checksums.txt
ff_0.10.0_linux_amd64.tar.gz: OK
```

One honest limit: `checksums.txt` is not itself signed today, so verification proves your download matches what CI published with the release, not who published it. Pin a version and fetch over TLS from the releases page.

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

## Platforms

Release builds cover Linux, macOS, and Windows, each on amd64 and arm64 — the same six targets the install scripts and the tap select from. CI runs the full test suite on all three operating systems for every code change; the Windows leg is sharded four ways for wall-clock, not for coverage.

For the reader checking whether Windows is a real platform here: line endings follow git's own rules by construction — fufu reads and writes the worktree through gix's filter pipeline, so `core.autocrlf` and `.gitattributes` are honored the way git honors them. Long paths get no special handling — fufu neither sets nor works around `core.longpaths`, so a repository that needs it under git needs it under fufu too. And two integration suites — the `ff git` passthrough and commit signing — run on unix only today; the differential suites and everything else run on all three.

Next: the [tutorial](tutorial.md), or [adopting fufu](adopting.md) if you already have a repository.
