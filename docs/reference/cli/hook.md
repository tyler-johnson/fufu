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

Follow the printed activation steps: load the shell configuration, or restart and trust the agent integration. For Codex, follow the printed plugin registration command if automatic registration is unavailable or fails. Its selector uses the existing marketplace name. Hooks are per machine; [`ff init`](init.md) enables repository snapshots separately. [`ff doctor`](doctor.md) checks integration configuration. Captures require delivered events and successful snapshots, and exclude ignored untracked files, unsaved buffers, and oversized content.

## Files installed

Claude Code, Codex, Copilot CLI, and Cursor receive managed plugin directories. OpenCode receives `plugins/fufu.js` and a skill directory; an existing `fufu.js` without fufu's ownership header is refused. Qwen Code receives settings entries, and shells receive marked rc-file blocks. The [client references](../hooks/index.md) list paths and activation steps.

Client installs include the shipped skill except Qwen Code and Claude's `--settings` mode. Owned plugin and skill directories are managed content and may be replaced during updates; keep personal files elsewhere.

Installers migrate supported older fufu configuration after verifying the replacement. Unrelated settings, marketplace entries, and hand-written shell equivalents remain. JSON commands matching fufu's current or retired spellings are managed even when pasted by hand. See each client reference for migration and removal details.

Use [`ff unhook`](unhook.md) to remove managed integrations.
