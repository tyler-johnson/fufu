<a id="ff-hook-copilot"></a>
# Copilot CLI

[`ff hook copilot`](../cli/hook.md) installs a snapshot plugin and the shipped skill for GitHub Copilot CLI.

## Install

```sh
ff hook copilot
```

## Activate

Copilot loads a plugin from a local marketplace live, so the next session picks it up; there is no trust step. `copilot plugin list` shows it as `fufu@fufu-ff` (enabled).

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

These inspect installed files only. Confirm real capture with the [capture-and-recovery check](../../agents/setup.md#verify).

<a id="what-it-writes"></a>
## Files changed

The installer writes an Agent Plugins 1.0 plugin at `~/.agents/plugins/copilot/fufu/` — a root `plugin.json` under the `agent-plugins.org` schema, `com.github.copilot/hooks/hooks.json` in Copilot's flat shape, and the skill under `skills/fufu/` — merges one entry into `~/.agents/plugins/copilot/marketplace.json`, and registers both in `~/.copilot/settings.json` under `enabledPlugins` and `extraKnownMarketplaces`. The `copilot/` namespace keeps these apart from Codex's incompatible manifest and marketplace one directory up. Other settings keys and other marketplace entries survive under the [shared ownership rules](index.md#files-changed); a settings file whose `enabledPlugins` or `extraKnownMarketplaces` is not an object is refused untouched.

```console
$ ff hook copilot
copilot plugin and skill written in ~/.agents/plugins/copilot/fufu; registered live as fufu@fufu-ff

$ find ~/.agents/plugins/copilot -type f | sort
~/.agents/plugins/copilot/fufu/com.github.copilot/hooks/hooks.json
~/.agents/plugins/copilot/fufu/plugin.json
~/.agents/plugins/copilot/fufu/skills/fufu/SKILL.md
~/.agents/plugins/copilot/marketplace.json

$ cat ~/.agents/plugins/copilot/fufu/com.github.copilot/hooks/hooks.json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "\"/usr/local/bin/ff\" trigger copilot",
        "env": {
          "FF_HOOK_EVENT": "preToolUse"
        },
        "timeoutSec": 30
      }
    ],
    "userPromptSubmitted": [
      {
        "type": "command",
        "bash": "\"/usr/local/bin/ff\" trigger copilot",
        "env": {
          "FF_HOOK_EVENT": "userPromptSubmitted"
        },
        "timeoutSec": 30
      }
    ],
    "sessionStart": [
      {
        "type": "command",
        "bash": "\"/usr/local/bin/ff\" trigger copilot",
        "env": {
          "FF_HOOK_EVENT": "sessionStart"
        },
        "timeoutSec": 30
      }
    ],
    "agentStop": [
      {
        "type": "command",
        "bash": "\"/usr/local/bin/ff\" trigger copilot",
        "env": {
          "FF_HOOK_EVENT": "agentStop"
        },
        "timeoutSec": 30
      }
    ],
    "sessionEnd": [
      {
        "type": "command",
        "bash": "\"/usr/local/bin/ff\" trigger copilot",
        "env": {
          "FF_HOOK_EVENT": "sessionEnd"
        },
        "timeoutSec": 30
      }
    ]
  }
}

$ cat ~/.agents/plugins/copilot/marketplace.json
{
  "name": "fufu-ff",
  "owner": {
    "name": "fufu"
  },
  "plugins": [
    {
      "name": "fufu",
      "source": "./fufu"
    }
  ]
}

$ cat ~/.copilot/settings.json
{
  "enabledPlugins": {
    "fufu@fufu-ff": true
  },
  "extraKnownMarketplaces": {
    "fufu-ff": {
      "source": {
        "source": "directory",
        "path": "~/.agents/plugins/copilot"
      }
    }
  }
}
```

Each hook entry runs the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, double-quoted, and sets `FF_HOOK_EVENT` in its environment, because Copilot's payload names no event. `preToolUse` attempts capture before every tool call; `userPromptSubmitted` captures and delivers the briefing; `sessionStart` rebriefs; `agentStop` captures the final edit of a turn; `sessionEnd` captures once more at the end. The briefing arrives as `additionalContext` JSON. Copilot captures and tallies recognized Git writes only where the pre-tool payload carries a recognizable tool and command; it returns no pre-tool coaching or denial reply, including under strict policy. The manifest's version is the fufu version plus `+ff.` and eight hex digits of the hooks file's SHA-256.

The marketplace root is shared with tower. When tower created the file, fufu keeps its name and owner, appends its own entry, and registers as `fufu@<that name>`; the installer's last line says which selector to expect.

<a id="what-ff-unhook-copilot-removes"></a>
## Remove

[`ff unhook copilot`](../cli/unhook.md) removes the plugin directory, fufu's marketplace entry, and the `enabledPlugins` key. The marketplace file and its `extraKnownMarketplaces` registration go only when no other plugin is listed there.

```console
$ ff unhook copilot
copilot removed ~/.agents/plugins/copilot/fufu and registration fufu@fufu-ff

$ find ~/.agents/plugins/copilot -type f | sort

$ cat ~/.copilot/settings.json
{
  "enabledPlugins": {},
  "extraKnownMarketplaces": {}
}
```

<a id="notes"></a>
## Troubleshooting and migration

If `copilot plugin list` does not show the plugin, check that `~/.copilot/settings.json` still carries both keys — `ff hook -l` reports `partial — Copilot registration missing` when it does not — and run `ff hook -u`. A marketplace file another tool rewrote whole reads as `partial — marketplace entry missing`, which `ff hook -u` repairs too. `ff doctor --fix` repairs partial or stale managed configuration and a drifted skill.
