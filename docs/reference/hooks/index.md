# Hooks

Install hooks with [`ff hook`](../cli/hook.md) so your shell or agent client attempts snapshots as you work. Choose the clients you use on this machine:

## Install

```sh
ff hook bash             # Shell alias and prompt snapshots
ff hook claude           # Claude Code hooks and skill
ff hook -l               # List installed configuration
```

- Shells: [Bash](bash.md), [Zsh](zsh.md), [Fish](fish.md), [PowerShell](powershell.md).
- Agent clients: [Claude Code](claude.md), [Codex](codex.md), [Qwen Code](qwen.md), [Cursor](cursor.md).

Hook installation changes local files and makes no network request.

## Activate

Restart the shell or source the file named by the installer; each shell page gives the exact command. Restart Claude Code after installing its plugin. For Codex, the installer runs `codex plugin add fufu@fufu` when `codex` is on `PATH` and names the command otherwise; then run `/hooks` and review and approve the hook by hash after installation or changes. Restart Cursor or Qwen Code before testing their new configuration.

Cursor cloud agents do not fire `sessionStart`, so they receive no fufu briefing; their `preToolUse` event can still capture. Cursor has no installed turn-end capture event, so a final edit there waits for the next matching event or repository command; Codex and Qwen Code install `Stop` and `SessionEnd`. See [agent setup](../../agents/setup.md#what-the-hook-captures-and-what-it-tells-the-agent) for each client's events and replies.

## Verify

Run `ff hook -l` and [`ff doctor --no-fetch`](../cli/doctor.md). They inspect installed files, not a running shell, loaded plugin, or Codex trust approval. Use the checks on each shell page or the [agent capture-and-recovery check](../../agents/setup.md#verify) to verify actual events.

Shell hooks add a `git` alias or function that routes through [`ff git`](../cli/git.md), plus a prompt hook running [`ff trigger shell`](../cli/trigger.md). Allowed passthrough commands attempt a snapshot before Git runs; strict-policy refusals happen before that capture. Prompt snapshots are quiet. Recovery still depends on a successful retained snapshot; [coverage and limits](../../concepts/snapshots-and-undo.md#coverage-and-limits) apply.

## Files changed

- **Shells:** marked lines in the shell's startup file. Alias and prompt hook are managed independently. Recognized hand-written equivalents are reported and left alone; the missing piece can still be installed.
- **Agent settings:** fufu merges recognized hook commands into JSON. Unrelated settings and commands survive, although formatting can change. Invalid JSON is refused without rewriting that file. A command matching fufu's current or retired spelling is treated as managed even if you pasted it by hand.
- **Owned directories:** the Claude Code and Codex plugin directories are written and removed as fufu-managed content, the skill inside each. Keep personal files elsewhere. Codex also merges one entry into `~/.agents/plugins/marketplace.json`, which follows the agent-settings rules above. Cursor and Qwen Code receive no skill from these installers.

Re-running an installer repairs missing or stale managed entries and refreshes shipped content. `ff hook -u` refreshes already installed integrations without adding new clients.

## Remove

```sh
ff unhook bash
ff unhook claude
```

[`ff unhook`](../cli/unhook.md) removes managed lines, entries, and owned directories for the named client. Unrelated settings and hand-written shell hooks survive. Restart the shell or client afterward: removing configuration does not unload it from an existing process.

## Troubleshooting and migration

`ff doctor --fix` repairs partial or stale managed hooks and stale skills. Activation and trust remain separate steps. If snapshots are absent, check the running shell/client and the [verification recipe](../../agents/setup.md#verify) before relying on an installed-file row.

Retired shell markers and trigger spellings are recognized and upgraded by the installer. Managed MCP registrations from before v0.15 are removed by `ff hook <client>` or `ff hook -u` (and by `ff unhook cursor`); unrelated MCP commands and unmarked Codex TOML entries survive. `ff hook codex` also strips the entries and skill an earlier fufu wrote under `~/.codex/`. Gemini CLI is no longer a slug: `~/.gemini` is never touched, and entries there keep capturing through the retired `ff trigger gemini` source. Client pages identify the affected files.
