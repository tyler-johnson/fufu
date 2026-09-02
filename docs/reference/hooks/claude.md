# ff hook claude

A plugin directory at `~/.claude/skills/fufu/`, which fufu owns outright: written whole, removed whole, with nothing of yours inside it. Claude Code loads a plugin from that location with no marketplace and no install step. The directory holds four files.

- `.claude-plugin/plugin.json`, the manifest: the plugin's name `fufu`, the version of the fufu that wrote it, a one-line description, and the repository as its homepage.
- `hooks/hooks.json`, the seven events below.
- `skills/fufu/SKILL.md`, [fufu's skill](../../agents/setup.md), the manual an agent reads for recovery, rewriting closed commits, and the JSON. `ff hook --skill` prints the same text.
- `.mcp.json`, the [`ff mcp`](../cli/mcp.md) server, so the agent has fufu as a tool named `mcp__plugin_fufu_fufu__ff`.

## What it writes

```console
$ ff hook claude
claude plugin written to ~/.claude/skills/fufu
  skill written to ~/.claude/skills/fufu/skills/fufu
  MCP server registered in ~/.claude/skills/fufu/.mcp.json
  restart Claude Code to load it (`claude plugin list` shows it as fufu@skills-dir)

$ find ~/.claude/skills/fufu -type f | sort
~/.claude/skills/fufu/.claude-plugin/plugin.json
~/.claude/skills/fufu/.mcp.json
~/.claude/skills/fufu/hooks/hooks.json
~/.claude/skills/fufu/skills/fufu/SKILL.md

$ cat ~/.claude/skills/fufu/.mcp.json
{
  "mcpServers": {
    "fufu": {
      "type": "stdio",
      "command": "/usr/local/bin/ff",
      "args": [
        "mcp"
      ]
    }
  }
}

$ cat ~/.claude/skills/fufu/hooks/hooks.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write|NotebookEdit",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
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
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ],
    "SubagentStop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ],
    "SubagentStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ],
    "CwdChanged": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/ff trigger claude"
          }
        ]
      }
    ]
  }
}
```

The command is the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, plus `trigger claude`; the server entry carries the same path with the one argument `mcp`. A plugin's hooks do not go looking on `PATH`, so the path is baked in. After moving or reinstalling the binary somewhere else, run `ff hook claude` again; the moved plugin still reads as wired and registered in the meantime, because fufu recognizes its own command by its tail.

`PreToolUse` and `UserPromptSubmit` are the floor: the snapshot before every tool call, and the turn boundary the briefing rides. The other five widen capture. `SessionStart` rebuilds the briefing after a resume, a `/clear`, a compaction, or a fork dropped the context it was in. `Stop` and `SubagentStop` make the last edit of a turn durable, since capture is a snapshot before an action and a session that ends on an edit would otherwise never snapshot it. `SubagentStart` lays a floor before a subagent writes anything, and `CwdChanged` lays one in the repository the agent just entered. A plugin missing one of the five is stale, which `ff doctor --fix` repairs; a plugin missing one of the two is partial, which is a finding.

Claude Code loads the plugin on its next restart. `claude plugin list` shows it as `fufu@skills-dir`.

`ff hook claude --settings` is the escape hatch: it merges the same seven events into `~/.claude/settings.json` with the command `ff trigger claude`, carries no skill and no server, and removes the plugin if there is one. The plugin is the mechanism a bare `ff hook claude` prefers. On a machine wired through settings entries, `ff hook claude` writes the plugin, verifies it, and only then strips the entries, so there is never a moment with no capture. Entries written under the older spellings `ff hook agent trigger claude` and `ff hook claude` are recognized as fufu's and upgraded in place.

## What `ff unhook claude` removes

The plugin directory, the server registration inside it with it, and any fufu entries in `~/.claude/settings.json`, whichever of the two an earlier install wrote. Both are checked every time.

```console
$ ff unhook claude
claude removed ~/.claude/skills/fufu

$ find ~/.claude -type f | sort
```

Settings entries written by hand that run something other than `ff trigger claude` stay.

## Notes

The two-event floor for a settings file you manage yourself, `PreToolUse` and `UserPromptSubmit` with the command `ff trigger claude`, is on [the setup page](../../agents/setup.md). It captures and briefs; it does not carry the skill, the server, or the five wider events. To register the server by hand instead, the same `mcpServers.fufu` entry goes in `~/.claude.json`, the user-scope file `claude mcp add --scope user` writes; the tool is then `mcp__fufu__ff`.

The `PreToolUse` matcher stays `Bash|Edit|Write|NotebookEdit` and does not name the MCP tool: every call through the server is a child `ff` that captures for itself, and a hook on it would capture twice. It is also why the `fufu.toolPolicy` refusal lands on `Bash` and not on the tool — the thing being refused is `ff` typed into the shell while the tool is up, and `Bash` is where that happens.

In a script, give `ff hook claude` a closed stdin (`< /dev/null`). When stdin is a pipe rather than a terminal, the command first looks there for a hook payload, because `ff hook claude` was once the spelling that meant trigger and a stale hook entry may still run it.
