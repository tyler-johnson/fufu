# ff hook gemini

Two entries merged into `~/.gemini/settings.json`, in the nested shape Claude Code and Codex share. The file belongs to you and holds the rest of Gemini CLI's settings beside the hooks: fufu parses it, adds its entries, and writes everything else back untouched. No skill: Gemini CLI reads none.

## What it writes

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

`BeforeTool` is the snapshot before a shell command, a file write, or a replace; `SessionStart` is where the briefing lands. Entries already in the file that run something else stay, and so does every other setting in the file. A file that is not valid JSON is refused untouched. Running `ff hook gemini` on a wired file reports it as already wired and changes nothing.

## What `ff unhook gemini` removes

The two entries.

```console
$ ff unhook gemini
gemini removed from ~/.gemini/settings.json

$ cat ~/.gemini/settings.json
{}
```

An entry that carried a foreign command beside fufu's keeps the foreign command. An event left with no entries is dropped, and a `hooks` object left with no events is dropped too. On a real machine the file keeps whatever else it held; it is empty above because the transcript started from an empty one.
