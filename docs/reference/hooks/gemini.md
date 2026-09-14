<a id="ff-hook-gemini"></a>
# Gemini CLI

[`ff hook gemini`](../cli/hook.md) installs snapshot hooks and session briefings for Gemini CLI.

## Install

```sh
ff hook gemini
```

## Activate

Restart Gemini CLI to load the configuration. Ensure `ff` is on the client's PATH.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

[`ff doctor`](../cli/doctor.md) checks installed files. Use the [capture-and-recovery check](../../agents/setup.md#verify) to confirm the running client sends events; an installed-file `ok` does not prove activation.

<a id="what-it-writes"></a>
## Files changed

The installer merges two nested hook events into `~/.gemini/settings.json`. Unrelated settings and commands survive under the [shared ownership rules](index.md#files-changed). This installer supplies no skill.

```console
$ ff hook gemini
gemini wired into ~/.gemini/settings.json

$ cat ~/.gemini/settings.json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command|write_file|replace",
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger gemini"
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger gemini"
          }
        ]
      }
    ]
  }
}
```

`BeforeTool` attempts capture before run_shell_command, write_file, or replace calls; `SessionStart` delivers the briefing. There is no installed turn-end event, so a final edit waits for another event or repository command. Gemini CLI captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-gemini-removes"></a>
## Remove

[`ff unhook gemini`](../cli/unhook.md) removes the managed commands. Restart Gemini CLI afterward.

```console
$ ff unhook gemini
gemini removed from ~/.gemini/settings.json

$ cat ~/.gemini/settings.json
{}
```

An entry that carried a foreign command beside fufu's keeps the foreign command. An event left with no entries is dropped, and a `hooks` object left with no events is dropped too. On a real machine the file keeps whatever else it held; it is empty above because the transcript started from an empty one.

## Troubleshooting and migration

If capture is absent, check PATH, restart Gemini CLI, and run the verification recipe. `ff doctor --fix` repairs partial or stale managed entries.

Before v0.15, fufu registered its MCP command as `mcpServers.fufu` in this settings file. Installation, refresh, or removal deletes that managed registration. An entry running an unrelated command survives.
