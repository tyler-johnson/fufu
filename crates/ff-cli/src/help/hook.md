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

Supported slugs are `claude`, `codex`, `qwen`, `opencode`, `cursor`, `bash`, `zsh`, `fish`, and `powershell`. Cursor means the agent client. Gemini CLI is retired as a slug; `ff trigger gemini` still answers entries an earlier fufu wrote. PowerShell supports PowerShell 7 and Windows PowerShell 5.1 through `$PROFILE`, a Git function, and a prompt wrapper.

`--all` installs detected integrations without asking. `-l` (`--list`) only reports. `-u` (`--update`) refreshes existing installations using their current mechanisms and adds no new clients. Install scripts run this refresh after updating the binary. `--skill` prints the instructions without installing hooks.

Follow the printed activation steps: load the shell configuration, or restart and trust the agent integration. For Codex the installer runs `codex plugin add fufu@fufu` when `codex` is on `PATH` and otherwise prints it. Hooks are per machine; `ff init` enables repository snapshots separately. `ff doctor` checks integration configuration. Captures require delivered events and successful snapshots, and exclude ignored untracked files, unsaved buffers, and oversized content.

### Files installed

Claude Code receives a managed plugin directory under `~/.claude/skills/fufu/`. Codex receives one under `~/.agents/plugins/fufu/` plus an entry merged into `~/.agents/plugins/marketplace.json`. OpenCode receives one plugin module, `plugins/fufu.js` under its config directory, with the skill beside it; a `fufu.js` fufu did not write is refused and left alone. Qwen Code and Cursor receive entries merged into their settings; shells receive marked rc-file blocks. Recognized hand-written shell equivalents are left alone. JSON commands matching fufu's current or retired spellings are managed even when pasted by hand; unrelated commands survive.

The Claude Code, Codex, and OpenCode installs carry the shipped skill. Claude's `--settings` mode installs settings-based capture instead of the plugin and does not install its skill. Owned plugin and skill directories are replaced as managed content. The Codex install strips the entries and skill an earlier fufu wrote under `~/.codex/`, and old managed MCP registrations are removed; unrelated commands and unmarked Codex TOML entries remain.

Use `ff unhook` to remove managed integrations.
