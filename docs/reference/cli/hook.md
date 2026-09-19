# ff hook

Install fufu's shell and agent integrations on this machine. With no arguments, report detected clients and shells, then ask which to install. Installed and active hooks attempt snapshots at received agent events, typed Git commands through the alias, and shell prompts.

## Usage

```
Usage: ff hook [OPTIONS] [slug]...
```

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

## Options

```
Arguments:
  [slug]...
          Slugs to hook: claude, codex, qwen, opencode, copilot, cursor, bash, zsh, fish, powershell

Options:
      --all
          Everything detected, without asking

  -l, --list
          Report what is here and stop

      --settings
          Claude only: install settings entries instead of the plugin

      --json
          Emit machine-readable JSON

  -u, --update
          Refresh existing integrations without adding new ones

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --skill
          Print the shipped agent instructions without installing

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Selection and activation

Supported slugs are `claude`, `codex`, `qwen`, `opencode`, `copilot`, `cursor`, `bash`, `zsh`, `fish`, and `powershell`. Cursor means the agent client. Gemini CLI is retired as a slug; [`ff trigger gemini`](trigger.md) still answers entries an earlier fufu wrote. PowerShell supports PowerShell 7 and Windows PowerShell 5.1 through `$PROFILE`, a Git function, and a prompt wrapper.

`--all` installs detected integrations without asking. `-l` (`--list`) only reports. `-u` (`--update`) refreshes existing installations using their current mechanisms and adds no new clients. Install scripts run this refresh after updating the binary. `--skill` prints the instructions without installing hooks.

Follow the printed activation steps: load the shell configuration, or restart and trust the agent integration. For Codex the installer runs `codex plugin add fufu@fufu` when `codex` is on `PATH` and otherwise prints it. Hooks are per machine; [`ff init`](init.md) enables repository snapshots separately. [`ff doctor`](doctor.md) checks integration configuration. Captures require delivered events and successful snapshots, and exclude ignored untracked files, unsaved buffers, and oversized content.

## Files installed

Claude Code receives a managed plugin directory under `~/.claude/skills/fufu/`. Codex receives one under `~/.agents/plugins/fufu/` plus an entry merged into `~/.agents/plugins/marketplace.json`. OpenCode receives one plugin module, `plugins/fufu.js` under its config directory, with the skill beside it; a `fufu.js` fufu did not write is refused and left alone. Copilot CLI receives an Agent Plugins 1.0 plugin under `~/.agents/plugins/copilot/fufu/`, an entry in the marketplace file beside it, and a registration merged into `~/.copilot/settings.json`. Cursor receives a user-local plugin under `~/.cursor/plugins/local/fufu/`. Qwen Code receives entries merged into its settings; shells receive marked rc-file blocks. Recognized hand-written shell equivalents are left alone. JSON commands matching fufu's current or retired spellings are managed even when pasted by hand; unrelated commands survive.

Every client install but Qwen Code's carries the shipped skill. Claude's `--settings` mode installs settings-based capture instead of the plugin and does not install its skill. Owned plugin and skill directories are replaced as managed content. The Codex install strips the entries and skill an earlier fufu wrote under `~/.codex/`, the Cursor install the entries it merged into `~/.cursor/hooks.json`, and old managed MCP registrations are removed; unrelated commands and unmarked Codex TOML entries remain.

Use [`ff unhook`](unhook.md) to remove managed integrations.
