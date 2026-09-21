<a id="ff-hook-copilot"></a>
# Copilot CLI

[`ff hook copilot`](../cli/hook.md) installs a snapshot plugin and the shipped skill for GitHub Copilot CLI.

## Install

```sh
ff hook copilot
```

## Activate

Start a new Copilot session after installation; no separate hook trust step is required. Check `copilot plugin list` for the enabled selector printed by the installer. It is `fufu@fufu-ff` for a new marketplace, or `fufu@<existing-name>` when the marketplace already has a name.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

These inspect installed files only. Confirm real capture with the [capture-and-recovery check](../../agents/setup.md#verify).

<a id="what-it-writes"></a>
## Files changed

The installer writes these managed files:

- The plugin under `~/.agents/plugins/copilot/fufu/`, including its manifest, `com.github.copilot/hooks/hooks.json`, and `skills/fufu/`.
- One entry in `~/.agents/plugins/copilot/marketplace.json`.
- Registration entries under `enabledPlugins` and `extraKnownMarketplaces` in `~/.copilot/settings.json`.

The Copilot directory is separate from Codex's plugin and marketplace. Other entries follow the [shared ownership rules](index.md#files-changed). A malformed settings file is left untouched, though other installation files may already have been written.

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

Each hook runs the installing binary's absolute path, double-quoted, and supplies the event name through `FF_HOOK_EVENT`. Copilot's payload does not identify the event.

`preToolUse` attempts capture before every tool call. `userPromptSubmitted` captures and sends the briefing as `additionalContext` JSON; `sessionStart` rebriefs. `agentStop` and `sessionEnd` attempt capture at the end of a turn or session. Recognizable Git commands are tallied, but the adapter sends no pre-tool coaching or denial reply, including under strict policy.

The plugin version includes a hash of its hooks file so changed hook definitions produce a new version.

The marketplace root is shared with tower. When tower created the file, fufu keeps its name and owner, appends its own entry, and registers as `fufu@<that name>`; the installer's last line says which selector to expect. With only tower's plugin under the root, `ff hook -l` reports `not wired`.

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
