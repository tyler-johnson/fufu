<a id="ff-hook-claude"></a>
# Claude Code

[`ff hook claude`](../cli/hook.md) installs snapshot hooks, session briefings, and the shipped skill as a Claude Code plugin. Restart Claude Code to load it.

## Install

```sh
ff hook claude
```

## Activate

Exit and restart Claude Code. It loads the plugin from `~/.claude/skills/fufu/` without a marketplace registration or a separate plugin-install command.

## Verify

```sh
claude plugin list
ff hook -l
ff doctor --no-fetch
```

The plugin list should contain `fufu@skills-dir`. [`ff doctor`](../cli/doctor.md) checks installed files; use the [capture-and-recovery check](../../agents/setup.md#verify) to confirm a running session sends events.

<a id="what-it-writes"></a>
## Files changed

Fufu manages the whole `~/.claude/skills/fufu/` directory. Keep personal files outside it. It contains `.claude-plugin/plugin.json` (manifest), `hooks/hooks.json` (seven events), and `skills/fufu/SKILL.md` ([the shipped skill](../../agents/setup.md#ship-the-skill)). `ff hook --skill` prints the same skill text.

```console
$ ff hook claude
claude plugin written to ~/.claude/skills/fufu
  skill written to ~/.claude/skills/fufu/skills/fufu
  restart Claude Code to load it (`claude plugin list` shows it as fufu@skills-dir)

$ find ~/.claude/skills/fufu -type f | sort
~/.claude/skills/fufu/.claude-plugin/plugin.json
~/.claude/skills/fufu/hooks/hooks.json
~/.claude/skills/fufu/skills/fufu/SKILL.md

$ cat ~/.claude/skills/fufu/hooks/hooks.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write|NotebookEdit",
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "matcher": "startup|resume|clear|compact|fork",
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "SubagentStop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "SubagentStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ],
    "CwdChanged": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"/usr/local/bin/ff\" trigger claude"
          }
        ]
      }
    ]
  }
}
```

The command is the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, plus `trigger claude`. A plugin's hooks do not go looking on `PATH`, so the path is baked in. The path is always double-quoted: Claude Code runs the command through Git Bash on Windows, where an unquoted `C:\Users\…\ff.exe` collapses to `C:Users…ff.exe`.

### The seven events

`PreToolUse` attempts a snapshot before matching Bash, Edit, Write, and NotebookEdit calls. `UserPromptSubmit` captures at prompt submission and can deliver the briefing. The other events cover more session boundaries:

- `SessionStart` rebriefs at startup, resume, clear, compaction, and fork.
- `Stop` and `SubagentStop` attempt to capture the final edit of a turn.
- `SubagentStart` and `CwdChanged` attempt capture before subagent work and on entering a repository.

Pre-tool replies can also brief newly entered repositories or subagents. Claude Code is the adapter that returns Git-policy coaching and denial replies; [policy settings](../../agents/setup.md#pick-a-git-policy) control them. Capture precedes policy evaluation and remains subject to [coverage and contention limits](../../concepts/snapshots-and-undo.md#coverage-and-limits).

### The settings escape hatch

`ff hook claude --settings` merges the seven events into `~/.claude/settings.json` using [`ff trigger claude`](../cli/trigger.md), installs no skill, and removes an existing plugin. Unrelated settings and commands survive under the [shared ownership rules](index.md#files-changed).

To switch back, run `ff hook claude`. It writes and verifies the plugin before removing managed settings entries. Restart Claude Code after changing mechanisms; file installation alone does not establish continuous capture in the running client.

<a id="what-ff-unhook-claude-removes"></a>
## Remove

[`ff unhook claude`](../cli/unhook.md) removes the plugin directory and managed entries in `~/.claude/settings.json`. Both locations are checked. Restart Claude Code afterward.

```console
$ ff unhook claude
claude removed ~/.claude/skills/fufu

$ find ~/.claude -type f | sort
```

Settings entries written by hand that run something other than `ff trigger claude` stay.

<a id="notes"></a>
## Troubleshooting and migration

After moving the binary, run `ff hook claude` or `ff hook -u` to refresh the plugin's absolute command path, then restart Claude Code. Its old command can still be recognized as installed even when the binary has moved.

A missing primary event is partial; missing wider events or retired spellings are stale. `ff doctor --fix` repairs these installed-file findings. The [manual two-event settings example](../../agents/setup.md#manual-hook-configuration) captures and briefs but omits the skill and wider events.

Installers recognize the retired `ff hook agent trigger claude` and `ff hook claude` commands as managed. They also remove the pre-v0.15 plugin's `.mcp.json`, which registered the retired MCP command.

In a script, give `ff hook claude` a closed stdin (`< /dev/null`). When stdin is a pipe rather than a terminal, the command first looks there for a hook payload, because `ff hook claude` was once the spelling that meant trigger and a stale hook entry may still run it.
