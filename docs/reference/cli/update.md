# ff update

Moves the running binary to the latest release: picks this platform's asset, streams it through sha256 against the release's checksums.txt, and atomically renames it over the executable. Installs that are not fufu's to touch get pointed at their own updater instead — Homebrew at `brew upgrade fufu`, source builds at `cargo install`.

Official builds also keep themselves fresh without being asked. A check runs at most once per fufu.updateCheck (daily by default), and a newer release either installs itself silently in the background (fufu.autoUpdate, on by default) or lands a one-line notice on stderr instead. A release is announced at most once, ever.

--check is that background lane: it refreshes the cache and prints nothing.

## Usage

```
Usage: ff update [OPTIONS]

Options:
      --check
          Refresh the update cache only (used by the background check)

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff update                      update now
ff config autoUpdate false     keep checking, but only notice
ff config updateCheck false    turn the whole lane off
```
