<a id="ff-hook-qwen"></a>
# Qwen Code

[`ff hook qwen`](../cli/hook.md) installs snapshot hooks and session briefings for Qwen Code.

## Install

```sh
ff hook qwen
```

## Activate

Restart Qwen Code to load the configuration. Ensure `ff` is on the client's PATH.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

These inspect installed files only. Confirm real capture with the [capture-and-recovery check](../../agents/setup.md#verify).

<a id="what-it-writes"></a>
## Files changed

The installer merges five nested hook events into `~/.qwen/settings.json`. Unrelated settings and commands survive under the [shared ownership rules](index.md#files-changed). This installer supplies no skill.

```console
$ ff hook qwen
qwen wired into ~/.qwen/settings.json

$ cat ~/.qwen/settings.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "run_shell_command|write_file|replace|edit",
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger qwen"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger qwen"
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger qwen"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger qwen"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger qwen"
          }
        ]
      }
    ]
  }
}
```

`PreToolUse` attempts capture before run_shell_command, write_file, replace, or edit calls; `UserPromptSubmit` captures and delivers the briefing; `SessionStart` rebriefs after a new or resumed session; `Stop` captures the final edit of a turn; `SessionEnd` captures once more at the end. Qwen Code reads injected context from `hookSpecificOutput.additionalContext`, which is how the briefing arrives. It captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-qwen-removes"></a>
## Remove

[`ff unhook qwen`](../cli/unhook.md) removes the managed commands. Restart Qwen Code afterward.

```console
$ ff unhook qwen
qwen removed from ~/.qwen/settings.json

$ cat ~/.qwen/settings.json
{}
```

<a id="notes"></a>
## Troubleshooting and migration

If capture is absent, check PATH, restart Qwen Code, and run the verification recipe. `ff doctor --fix` repairs partial or stale managed entries.

This adapter replaced Gemini CLI's. `ff hook gemini` and `ff unhook gemini` are unknown slugs now, and `~/.gemini/settings.json` is never touched; entries an earlier fufu wrote there keep capturing, because [`ff trigger gemini`](../cli/trigger.md) still answers under the source `gemini`. Remove them by hand when Gemini CLI goes.
