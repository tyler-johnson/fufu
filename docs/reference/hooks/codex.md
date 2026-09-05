# ff hook codex

Two entries merged into `~/.codex/hooks.json`, [fufu's skill](../../agents/setup.md) in a directory of its own at `~/.codex/skills/fufu/`, and the [`ff mcp`](../cli/mcp.md) server as a marked block in `~/.codex/config.toml`.

The hooks file belongs to you: fufu parses it, adds its entries, and writes everything else back untouched. The skill directory belongs to fufu, written whole and removed whole. The config file belongs to you too, and fufu carries no TOML parser, so the block is appended between two marker comments and removed by them, the way the shells take marked lines.

A declared extension's skills get a directory each the same way, `~/.codex/skills/<skill>/`, and a person mentions one as `$<skill>`. The manifest names the skills and the binary produces each one's files through `ff-<name> --ff-skill <skill>` when the install runs. A skill the binary will not produce is left out and said. [`ff hook --skill <skill>`](../../reference/cli/hook.md) prints a skill's `SKILL.md` without installing anything.

## What it writes

```console
$ ff hook codex
codex wired into ~/.codex/hooks.json
  skill written to ~/.codex/skills/fufu
  MCP server registered in ~/.codex/config.toml
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

$ cat ~/.codex/config.toml
# >>> fufu (ff hook codex) >>>
[mcp_servers.fufu]
command = "/usr/local/bin/ff"
args = ["mcp"]
# <<< fufu <<<

$ find ~/.codex -type f | sort
~/.codex/config.toml
~/.codex/hooks.json
~/.codex/skills/fufu/SKILL.md
```

`PreToolUse` is the snapshot before a shell command or a patch; `UserPromptSubmit` is the turn boundary the briefing rides. Entries already in the file that run something else stay, in whatever shape they had. A file that is not valid JSON is refused untouched. Running [`ff hook codex`](../../reference/cli/hook.md) on a wired file reports it as already wired and rewrites the skill, which is how a skill that has drifted from this fufu is refreshed.

The block in `config.toml` is the server: Codex reads `[mcp_servers.<name>]` tables, and this one runs the absolute path of the binary that ran `ff hook`, shown here as `/usr/local/bin/ff`, with the one argument `mcp`, so the tool is `fufu`'s `ff`. Anything else in the file stays where it was, above the block. A `[mcp_servers.fufu]` table outside the markers was written by hand and is reported and left alone.

The trust line is the one thing to act on. Codex trusts a hook by its hash: run `/hooks` in Codex to review this one, or it is skipped and nothing captures.

`ff hook -l` and [`ff doctor`](../../reference/cli/doctor.md) keep saying so for as long as the hook is wired, because fufu cannot read Codex's trust list and an unreviewed hook looks the same as a reviewed one from outside. The skill needs no review: it is a file Codex reads, not a command it runs.

## What `ff unhook codex` removes

The two entries, the skill directory, the marked block, and every skill of every extension still on the registry when it runs, by name and with no handshake — a skill of one this fufu no longer describes because it was taken back with `ff extension remove` first is left where it is.

```console
$ ff unhook codex
codex removed from ~/.codex/hooks.json
  removed ~/.codex/skills/fufu
  MCP server removed from ~/.codex/config.toml

$ cat ~/.codex/hooks.json
{}

$ find ~/.codex -type f | sort
~/.codex/config.toml
~/.codex/hooks.json
```

`config.toml` is left empty above because the transcript started from none; on a real machine everything outside the markers stays.

An entry that carried a foreign command beside fufu's keeps the foreign command. An event left with no entries is dropped, and a `hooks` object left with no events is dropped too, which is why the file above is empty rather than holding empty lists.

## Notes

[The setup page](../../agents/setup.md) covers Codex from the agent's side, including what the briefing says once the hook is trusted.
