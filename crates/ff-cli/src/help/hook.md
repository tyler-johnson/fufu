Install fufu's shell and agent integrations on this machine. With no arguments, report detected clients and shells, then ask which to install. Installed and active hooks attempt snapshots at received agent events, typed Git commands through the alias, and shell prompts.

## Examples

```sh
ff hook                         # Detect integrations and ask
ff hook claude codex             # Install selected clients
ff hook bash                    # Install shell integration
ff hook --all                   # Install every detected integration
ff hook -l                      # List without installing
ff hook -u                      # Refresh existing installations
ff hook --skill                 # Print the shipped agent instructions
```

### Options

### Selection and activation

Supported slugs are `claude`, `codex`, `cursor`, `gemini`, `bash`, `zsh`, `fish`, and `powershell`. Cursor means the agent client. PowerShell supports PowerShell 7 and Windows PowerShell 5.1 through `$PROFILE`, a Git function, and a prompt wrapper.

`--all` installs detected integrations without asking. `-l` (`--list`) only reports. `-u` (`--update`) refreshes existing installations using their current mechanisms and adds no new clients. Install scripts run this refresh after updating the binary. `--skill` prints the instructions without installing hooks.

Follow the printed activation steps: load the shell configuration, or restart and trust the agent integration. Hooks are per machine; `ff init` enables repository snapshots separately. `ff doctor` checks integration configuration. Captures require delivered events and successful snapshots, and exclude ignored untracked files, unsaved buffers, and oversized content.

### Files installed

Claude Code receives a managed plugin directory. Other clients receive entries merged into their settings; shells receive marked rc-file blocks. Existing hand-written integrations are detected and reported without replacement.

Claude Code and Codex also receive the shipped skill. Claude's `--settings` mode installs settings-based capture instead of the plugin and does not install its skill. An old fufu-managed MCP registration is removed on installation; hand-written registrations remain.

Use `ff unhook` to remove managed integrations.
