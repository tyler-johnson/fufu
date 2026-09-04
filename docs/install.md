# Install

## Get the binary

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
fufu 0.12.0 (6aa4efe 2026-09-04)
https://github.com/tyler-johnson/fufu
```

To install one exact release, or to check a download by hand, see [Pin and verify](#pin-and-verify) at the end of this page.

## Wire it in

Optional, and recommended.

```sh
ff hook
```

fufu captures only when something invokes it, so without hooks only an `ff` command takes a capture — the snapshot of your working tree that undo returns you to. With hooks a snapshot lands before every agent tool call, every git command you type, and every shell prompt.

What each surface gets:

- **A shell** — two marked lines in your rc file, `alias git='ff git'` and a prompt hook.
- **Windows** — [`ff hook powershell`](reference/cli/hook.md), which writes PowerShell's `$PROFILE`.
- **An agent client** — hook entries in its own settings file, or a plugin directory for Claude Code.

What each slug writes, and what [`ff unhook`](reference/cli/unhook.md) takes back, is on [the hook reference](reference/hooks/index.md).

Bare `ff hook` reports the shells and agent clients it found and asks; `--all` takes everything detected, `-l` reports and stops. Claude Code and Codex get [fufu's skill](agents/setup.md) with the wiring. Once per machine, not per repository.

## What you installed

fufu ships as a single `ff` binary. Most of what it does is native, but today it still reaches your git installation for a few operations — the push, credential helpers, hooks — so keep git installed. [`ff doctor`](reference/cli/doctor.md) reports what fufu found and whether the repository it is standing in is armed.

`ff update` names the command that updates this copy of fufu — the install script, `brew upgrade fufu`, `cargo install`, or whatever else placed the binary — and offers to run it. Nothing updates itself unasked.

## Platforms

Release builds cover Linux, macOS, and Windows, each on amd64 and arm64 — the same six targets the install scripts and the tap select from. CI runs the full test suite on all three operating systems for every code change; the Windows leg is sharded four ways for wall-clock, not for coverage.

### On Windows

Line endings follow git's own rules by construction: fufu reads and writes the worktree through gix's filter pipeline, so `core.autocrlf` and `.gitattributes` are honored the way git honors them. Long paths get no special handling — fufu neither sets nor works around `core.longpaths`, so a repository that needs it under git needs it under fufu too.

Four integration suites run on unix only today:

- the [`ff git`](reference/cli/git.md) passthrough
- the extension suite
- the zero-spawn proof
- commit signing

Everything else runs on all three, the differential suites included, and the PowerShell hook's profile is dot-sourced by a real `pwsh` on every one.

## Pin and verify

Both installers take a version: set `FF_VERSION=vX.Y.Z` (`$env:FF_VERSION` in PowerShell) and the script installs exactly that release instead of the latest. Either way the script verifies the download's sha256 against the `checksums.txt` published with the release before anything lands on your PATH.

To skip the scripts entirely, every release publishes versioned archives — `ff_<version>_<os>_<arch>.tar.gz`, `.zip` on Windows — beside their `checksums.txt` on the [releases page](https://github.com/tyler-johnson/fufu/releases). Download the archive and `checksums.txt` into one directory, verify, and put `ff` on your PATH:

```console
$ sha256sum -c --ignore-missing checksums.txt
ff_0.12.0_linux_amd64.tar.gz: OK
```

One honest limit: `checksums.txt` is not itself signed today, so verification proves your download matches what CI published with the release, not who published it. Pin a version and fetch over TLS from the releases page.

## In a regulated environment

The pieces above, as one list to file with security:

1. **Pin.** Set `FF_VERSION=vX.Y.Z` with the script, or take the versioned archive from the releases page. With the version pinned, `install.sh` fetches only the archive and `checksums.txt`, from the release's own download URL, and nothing else. `install.ps1` does the same, and asks the GitHub API for the latest tag only when no version is set.
2. **Verify, with the limit in the same breath.** The scripts check the sha256 against `checksums.txt` and refuse on a mismatch; by hand it is the `sha256sum -c` above. `checksums.txt` is unsigned, so this proves the download matches what CI published with the release, not who published it. Signed provenance is not offered today.
3. **Turn the update check off.** [`ff config --global updateCheck false`](reference/cli/config.md), which is the git config key [`fufu.updateCheck`](reference/config.md#updatecheck). What it turns off, in official builds:

    - at most once a day, a detached [`ff update --check`](reference/cli/update.md) makes one GET to `api.github.com` for the latest release tag;
    - it sends `GITHUB_TOKEN` as a bearer header if the environment has one;
    - it caches the answer in `<cache>/fufu/update.json` for a one-line notice.

    It never installs anything; `false` stops the check and the notice both.
4. **What remains.** With the check off, nothing in fufu itself reaches the network. `ff update` fetches only when you run it, `ff hook` writes local files and nothing else, and [`ff sync`](reference/cli/sync.md) and [`ff publish`](reference/cli/publish.md) talk only to the remotes your repository configures: the fetch is native and reads git's credential and proxy config, and the push runs git.

Next: the [tutorial](tutorial.md), or [adopting fufu](adopting.md) if you already have a repository.
