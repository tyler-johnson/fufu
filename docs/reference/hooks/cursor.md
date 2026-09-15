<a id="ff-hook-cursor"></a>
# Cursor

[`ff hook cursor`](../cli/hook.md) installs a snapshot plugin and the shipped skill for the Cursor agent client. **Cloud agents get no user-local hooks, so nothing captures there.**

## Install

```sh
ff hook cursor
```

## Activate

Start a new agent session in a trusted workspace; Cursor CLI discovers user-local plugins on session start, with no marketplace registration and no install command. Team policy must allow local plugin imports. A resumed chat keeps its context and fires no `sessionStart`.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

[`ff doctor`](../cli/doctor.md) checks installed files and retains the cloud-agent reminder. Test actual events with the [capture-and-recovery check](../../agents/setup.md#verify); an installed-file `ok` does not prove the running client loaded it.

<a id="what-it-writes"></a>
## Files changed

The installer writes the owned plugin directory `~/.cursor/plugins/local/fufu/` — the native `.cursor-plugin/plugin.json` manifest, `hooks/hooks.json` in Cursor's flat shape, and the skill under `skills/fufu/`. Other plugins under `plugins/local/`, `~/.cursor/hooks.json`, and Cursor's own settings are left alone. Personal files should stay outside the plugin directory.

```console
$ ff hook cursor
cursor plugin and skill written in ~/.cursor/plugins/local/fufu; Cursor discovers it on the next session
  Cursor loads local plugins in trusted workspaces, and team policy must allow local plugin imports; cloud agents get no user-local hooks, so nothing captures there

$ find ~/.cursor -type f | sort
~/.cursor/plugins/local/fufu/.cursor-plugin/plugin.json
~/.cursor/plugins/local/fufu/hooks/hooks.json
~/.cursor/plugins/local/fufu/skills/fufu/SKILL.md

$ cat ~/.cursor/plugins/local/fufu/hooks/hooks.json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "matcher": "Shell|Write|Delete",
        "command": "\"/usr/local/bin/ff\" trigger cursor"
      }
    ],
    "sessionStart": [
      {
        "command": "\"/usr/local/bin/ff\" trigger cursor"
      }
    ],
    "sessionEnd": [
      {
        "command": "\"/usr/local/bin/ff\" trigger cursor"
      }
    ]
  }
}
```

The command is the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, double-quoted. `preToolUse` attempts capture before Shell, Write, or Delete calls; `sessionStart` captures and delivers the briefing as `additional_context`; `sessionEnd` captures once more at the end. Cursor CLI 2026.09.10 gates `beforeSubmitPrompt` and `stop` on user or project hook settings even when a plugin declares them, so they are not wired; a final edit waits for `sessionEnd` or the next repository command. Hooks run from the plugin directory and name the workspace in `workspace_roots`, which is where the repository is discovered when the payload has no `cwd`. Cursor captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-cursor-removes"></a>
## Remove

[`ff unhook cursor`](../cli/unhook.md) removes the plugin directory, and strips the entries an earlier fufu merged into `~/.cursor/hooks.json` if any are still there. Start a new session afterward.

```console
$ ff unhook cursor
cursor removed ~/.cursor/plugins/local/fufu

$ find ~/.cursor -type f | sort
```

<a id="notes"></a>
## Troubleshooting and migration

If capture is absent, check that the workspace is trusted and that team policy allows local plugin imports, then run the verification recipe. `ff hook -u` restores a plugin missing an event; `ff doctor --fix` repairs partial or stale managed configuration and a drifted skill.

Before this plugin, fufu merged two flat entries into `~/.cursor/hooks.json`. That file still reads as wired, on the settings mechanism and stale, so `ff hook -u` migrates it: the plugin is written and verified first, then fufu's entries go and foreign entries beside them stay. A file that will not parse is reported and left. Before v0.15, fufu also registered an MCP server in `~/.cursor/mcp.json`; installation, refresh, or removal deletes that entry and leaves a hand-written one alone.
