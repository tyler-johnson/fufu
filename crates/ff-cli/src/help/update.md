Show how to update this installation of fufu. For an install-script binary, check the latest release and offer to run the installer when an update is available. Other installation types print their update instructions.

## Examples

```sh
ff update                       # Show the appropriate update command
ff update -y                    # Run an install-script update without asking
ff config updateCheck false      # Disable passive update checks
```

### Options

### Installation types

- Source builds receive a `cargo install` command.
- Homebrew installations receive `brew upgrade fufu`.
- Unmanaged binaries, including copies placed by mise or Nix, receive the releases-page URL.
- Binaries in the install script's destination receive that script's update command.

Only the install-script channel runs an installer. It runs after `-y` or an interactive yes. Without a terminal, it only prints instructions unless `-y` is given. `-y` on another channel returns an error. The installer replaces the binary and refreshes existing hooks with `ff hook -u`.

### Passive checks and output

Official builds can check for releases in the background on `fufu.updateCheck`'s cadence, daily by default. Update notices name the appropriate command; no update installs itself automatically. A cached release is announced at most once while that notice record remains.

`--check` refreshes the update cache silently, including silent lookup failures; it is used by the background check. Ordinary update output is human-oriented and is not wrapped by `--json`.
