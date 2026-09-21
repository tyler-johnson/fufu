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

Supported slugs are `claude`, `codex`, `qwen`, `opencode`, `copilot`, `cursor`, `bash`, `zsh`, `fish`, and `powershell`. Cursor means the agent client. Gemini CLI is retired as a slug; `ff trigger gemini` still answers entries an earlier fufu wrote. PowerShell supports PowerShell 7 and Windows PowerShell 5.1 through `$PROFILE`, a Git function, and a prompt wrapper.

`--all` installs detected integrations without asking. `-l` (`--list`) only reports. `-u` (`--update`) refreshes existing installations using their current mechanisms and adds no new clients. Install scripts run this refresh after updating the binary. `--skill` prints the instructions without installing hooks.

Follow the printed activation steps: load the shell configuration, or restart and trust the agent integration. For Codex, follow the printed plugin registration command if automatic registration is unavailable or fails. Its selector uses the existing marketplace name. Hooks are per machine; `ff init` enables repository snapshots separately. `ff doctor` checks integration configuration. Captures require delivered events and successful snapshots, and exclude ignored untracked files, unsaved buffers, and oversized content.

### Files installed

Claude Code, Codex, Copilot CLI, and Cursor receive managed plugin directories. OpenCode receives `plugins/fufu.js` and a skill directory; an existing `fufu.js` without fufu's ownership header is refused. Qwen Code receives settings entries, and shells receive marked rc-file blocks. The [client references](../hooks/index.md) list paths and activation steps.

Client installs include the shipped skill except Qwen Code and Claude's `--settings` mode. Owned plugin and skill directories are managed content and may be replaced during updates; keep personal files elsewhere.

Installers migrate supported older fufu configuration after verifying the replacement. Unrelated settings, marketplace entries, and hand-written shell equivalents remain. JSON commands matching fufu's current or retired spellings are managed even when pasted by hand. See each client reference for migration and removal details.

Use `ff unhook` to remove managed integrations.
