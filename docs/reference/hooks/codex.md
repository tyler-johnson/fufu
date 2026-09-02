# ff hook codex

Two entries merged into `~/.codex/hooks.json`, and [fufu's skill](../../agents/setup.md) in a directory of its own at `~/.codex/skills/fufu/`. The hooks file belongs to you: fufu parses it, adds its entries, and writes everything else back untouched. The skill directory belongs to fufu, written whole and removed whole.

## What it writes

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

`PreToolUse` is the snapshot before a shell command or a patch; `UserPromptSubmit` is the turn boundary the briefing rides. Entries already in the file that run something else stay, in whatever shape they had. A file that is not valid JSON is refused untouched. Running `ff hook codex` on a wired file reports it as already wired and rewrites the skill, which is how a skill that has drifted from this fufu is refreshed.

The trust line is the one thing to act on. Codex trusts a hook by its hash: run `/hooks` in Codex to review this one, or it is skipped and nothing captures. `ff hook -l` and `ff doctor` keep saying so for as long as the hook is wired, because fufu cannot read Codex's trust list and an unreviewed hook looks the same as a reviewed one from outside. The skill needs no review: it is a file Codex reads, not a command it runs.

## What `ff unhook codex` removes

The two entries, and the skill directory.

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

## Notes

[The setup page](../../agents/setup.md) covers Codex from the agent's side, including what the briefing says once the hook is trusted.
