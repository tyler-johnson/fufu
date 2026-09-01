# Install

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

fufu ships as a single `ff` binary. Most of what it does is native, but today it still reaches your git installation for a few operations — the push, credential helpers, hooks — so keep git installed. `ff doctor` reports what fufu found and whether the repository it is standing in is armed.

`ff update` updates fufu in place.

Next: the [tutorial](tutorial.md).
