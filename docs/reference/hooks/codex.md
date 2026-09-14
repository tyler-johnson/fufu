<a id="ff-hook-codex"></a>
# Codex

[`ff hook codex`](../cli/hook.md) installs snapshot hooks and the shipped skill for Codex. **Run `/hooks` in Codex and approve the hook by hash; an unreviewed hook is skipped.** Repeat the review after hook changes.

## Install

```sh
ff hook codex
```

## Activate

Start or restart Codex, run `/hooks`, and review and approve fufu's hook. The skill requires no command approval.

## Verify

```sh
ff hook -l
ff doctor --no-fetch
```

[`ff doctor`](../cli/doctor.md) cannot read Codex's trust state, so it always retains the review reminder. An installed-file `ok` does not prove approval or capture. Follow the [capture-and-recovery check](../../agents/setup.md#verify) in the running client.

<a id="what-it-writes"></a>
## Files changed

The installer merges two events into `~/.codex/hooks.json` and writes the owned `~/.codex/skills/fufu/` directory. Unrelated JSON entries survive under the [shared ownership rules](index.md#files-changed); personal files should stay outside the skill directory. Reinstallation refreshes the skill.

```console
$ ff hook codex
codex wired into ~/.codex/hooks.json
  skill written to ~/.codex/skills/fufu
  Codex trusts a hook by its hash: run /hooks in Codex to review this one, or it is skipped and nothing captures

$ cat ~/.codex/hooks.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|apply_patch",
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger codex"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ff trigger codex"
          }
        ]
      }
    ]
  }
}

$ find ~/.codex -type f | sort
~/.codex/hooks.json
~/.codex/skills/fufu/SKILL.md
```

`PreToolUse` attempts capture before Bash or apply_patch calls. `UserPromptSubmit` captures and can deliver the briefing. There is no installed turn-end or subagent event, so a final edit waits for the next event or repository command. Codex captures and tallies recognized Git writes but returns no pre-tool coaching or denial reply, including under strict policy.

<a id="what-ff-unhook-codex-removes"></a>
## Remove

[`ff unhook codex`](../cli/unhook.md) removes the managed commands and skill directory. Restart Codex afterward.

```console
$ ff unhook codex
codex removed from ~/.codex/hooks.json
  removed ~/.codex/skills/fufu

$ cat ~/.codex/hooks.json
{}

$ find ~/.codex -type f | sort
~/.codex/hooks.json
```

An entry that carried a foreign command beside fufu's keeps the foreign command. An event left with no entries is dropped, and a `hooks` object left with no events is dropped too, which is why the file above is empty rather than holding empty lists.

<a id="notes"></a>
## Troubleshooting and migration

If configuration is installed but no capture appears, check `/hooks` approval and that Codex can find `ff` on PATH. `ff doctor --fix` repairs partial or stale managed configuration and skills; it cannot approve a hook.

Before v0.15, fufu wrote a marked `[mcp_servers.fufu]` block into `~/.codex/config.toml`. Installation, refresh, or removal deletes that retired MCP block by its markers. An unmarked table is left alone.
