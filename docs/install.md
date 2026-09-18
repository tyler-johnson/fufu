# Install

Install `ff`, activate the integrations you want, then try the [tutorial](tutorial.md). Keep Git installed: fufu uses it for pushing, explicit Git commands, and some maintenance. Credential helpers and repository hooks can also need external programs; [Git dependencies](internals/substrate.md#the-git-free-destination) has the details.

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

Follow any PATH instructions printed by the installer so the shell can find `ff`.

<a id="wire-it-in"></a>

## Install hooks

[`ff hook`](reference/cli/hook.md) detects your shells and agent clients and asks which to configure. Hooks are optional and recommended: fufu captures only when invoked, and active hooks add capture attempts around supported tool calls, aliased Git commands, and shell prompts.

```sh
ff hook
```

Install hooks once per machine. This is separate from initializing each repository. `ff hook --all` selects everything detected; `ff hook -l` only lists the installed state. The [hook reference](reference/hooks/index.md) lists the supported clients and the files each integration writes.

## Activate the integrations

- **Bash, Zsh, Fish, or PowerShell:** restart the shell, or source the file named by `ff hook`. PowerShell can reload its profile with `. $PROFILE`.
- **Claude Code:** restart the client to load the plugin. `claude plugin list` should show `fufu@skills-dir`.
- **Codex:** run `/hooks` in Codex and review the installed hook. Codex skips an untrusted hook; fufu cannot read its trust list.
- **Other supported clients:** follow the instructions printed by `ff hook` and the client-specific [hook page](reference/hooks/index.md).

Installed files alone do not activate a running shell or make every editor and script invoke fufu. Recovery still depends on [successful snapshots, coverage, and retention](concepts/snapshots-and-undo.md#coverage-and-limits).

## Verify

Check that [`ff version`](reference/cli/version.md) runs. A released binary prints its version and build information, for example:

```console
$ ff version
fufu 0.17.0 (ffb1812 2026-09-18)
https://github.com/tyler-johnson/fufu
```

Then check the installed integrations:

```sh
ff hook -l
```

This reports files on disk; also complete the activation and trust steps above.

## Initialize or clone a repository

The [tutorial](tutorial.md) starts with [`ff clone`](reference/cli/clone.md) of fufu's own repository, which initializes fufu in the new checkout. Make a few edits there and walk through the everyday workflow; the push example is for a repository where you have push access.

For a repository you already use, run [`ff init`](reference/cli/init.md) inside it:

```sh
ff init
```

[Adopting fufu](adopting.md) explains existing edits and the workflow changes. After initialization, [`ff doctor`](reference/cli/doctor.md) reports repository setup and installed integrations. It also attempts capture/reconciliation, can fetch when enabled, and can run maintenance.

<a id="what-you-installed"></a>

## Update

[`ff update`](reference/cli/update.md) identifies how this binary was installed and prints the update command. For a binary in the install script's location, it checks for a release and offers to run the installer; `ff update -y` accepts that offer. Without an interactive terminal it only prints the command unless `-y` is supplied.

For Homebrew, source builds, and other locations, run the printed command or use the package manager that installed fufu. `ff update -y` refuses those channels. Automatic update checks only announce releases; they never install them.

The install scripts finish with `ff hook -u`, refreshing integrations already installed without adding new ones.

## Platforms

Release builds cover Linux, macOS, and Windows, each on amd64 and arm64. The install scripts and Homebrew tap select the matching binary. [How fufu is tested](project.md#how-it-is-tested) covers CI and platform-specific suites.

### On Windows

Line endings follow `core.autocrlf` and `.gitattributes` through gix's filter pipeline. Long paths receive no special handling: fufu neither sets nor works around `core.longpaths`, so a repository that needs it under Git needs it under fufu too.

## Pin and verify

Both installers accept an exact release through `FF_VERSION=vX.Y.Z` (`$env:FF_VERSION` in PowerShell). They verify the download's SHA-256 against the release's `checksums.txt` before installing the binary.

To install manually, download the versioned archive and `checksums.txt` from the [releases page](https://github.com/tyler-johnson/fufu/releases). Archives are named `ff_<version>_<os>_<arch>.tar.gz`, or `.zip` on Windows. Verify the archive, extract it, and put `ff` on your PATH. On Linux:

```console
$ sha256sum -c --ignore-missing checksums.txt
ff_0.17.0_linux_amd64.tar.gz: OK
```

`checksums.txt` is unsigned. Verification establishes that the download matches the published checksum, not who published it. Pin a version and fetch over TLS from the releases page.

<a id="in-a-regulated-environment"></a>

## Network settings

With `FF_VERSION` set, the installers download only the archive and checksums from that release. Without a pin, they also look up the latest release.

Official builds check for updates daily by default. To disable the automatic check and notices, use [`ff config`](reference/cli/config.md):

```sh
ff config --global updateCheck false
```

The check runs a detached `ff update --check`, requests the latest release from `api.github.com`, and caches results in `<cache>/fufu/update.json`. If `GITHUB_TOKEN` is set, it sends that token as a bearer header. An explicit `ff update` can still check for a release and run the installer as described above.

Repository traffic is separate. [`ff pull`](reference/cli/pull.md) fetches from remotes; [`ff push`](reference/cli/push.md) sends branch updates. Commands that read remote copies can auto-fetch first, on a ten-minute cadence by default. Disable automatic fetch with `ff config --global autoFetch false`, or suppress fetching for one supported invocation with `--no-fetch`. `--no-fetch` does not disable update checks. See [fetching and dry runs](concepts/push-boundary.md#fetching-and-dry-runs).

Native clone/fetch reads Git's credential helpers and `url.<base>.insteadOf`, but its HTTP backend does not honor `http.proxy`. Automatic fetch disables fufu's own credential prompt and times out after three seconds; a configured GUI helper can still prompt. Push and [`ff git`](reference/cli/git.md) use Git's transport. Hooks write local files and do not contact the network.
