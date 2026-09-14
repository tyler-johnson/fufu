<a id="ff-hook-cursor"></a>
# Cursor

[`ff hook cursor`](../cli/hook.md) installs snapshot hooks for the Cursor agent client. **Cloud agents do not fire `sessionStart`, so they receive no fufu briefing.** Their matching `preToolUse` events can still capture.

## Install

```sh
ff hook cursor
```

## Activate

Restart the Cursor agent client to load the configuration. Ensure `ff` is on the client's PATH.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

[`ff doctor`](../cli/doctor.md) checks installed files and retains the cloud-agent reminder. Test actual events with the [capture-and-recovery check](../../agents/setup.md#verify); an installed-file `ok` does not prove the running client loaded it.

<a id="what-it-writes"></a>
## Files changed

The installer merges two flat event entries into `~/.cursor/hooks.json` and adds `"version": 1` if absent. Unrelated settings and commands survive under the [shared ownership rules](index.md#files-changed). This installer supplies no skill.

```console
$ ff hook cursor
cursor wired into ~/.cursor/hooks.json
  Cursor does not fire sessionStart for cloud agents, so the briefing is absent there — capture still rides preToolUse

$ cat ~/.cursor/hooks.json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "matcher": "Shell|Write|Delete",
        "command": "ff trigger cursor"
      }
    ],
    "sessionStart": [
      {
        "command": "ff trigger cursor"
      }
    ]
  }
}
```

`preToolUse` attempts capture before Shell, Write, or Delete calls; `sessionStart` delivers the briefing. There is no installed turn-end event, so a final edit waits for another event or repository command. Cursor captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-cursor-removes"></a>
## Remove

[`ff unhook cursor`](../cli/unhook.md) removes the two managed entries. The `version` field stays. Restart the client afterward.

```console
$ ff unhook cursor
cursor removed from ~/.cursor/hooks.json

$ cat ~/.cursor/hooks.json
{
  "version": 1
}
```

<a id="notes"></a>
## Troubleshooting and migration

If capture is absent, check PATH, restart the agent client, and run the verification recipe. Missing briefings on cloud agents are an event limitation; reinstalling does not add `sessionStart` there. `ff doctor --fix` repairs partial or stale managed entries.

Before v0.15, fufu registered its MCP command as `mcpServers.fufu` in `~/.cursor/mcp.json`. Installation, refresh, or removal deletes that managed registration. An entry running an unrelated command survives.
