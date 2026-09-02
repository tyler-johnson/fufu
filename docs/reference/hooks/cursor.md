# ff hook cursor

Two entries merged into `~/.cursor/hooks.json`, in Cursor's flat shape: an entry is a matcher and a command, with no nested list. The file belongs to you: fufu parses it, adds its entries, sets `"version": 1` at the top level if it is absent, and writes everything else back untouched. No skill: Cursor reads none.

`cursor` is the agent client, not the editor. A future editor integration would get a slug of its own.

## What it writes

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

`preToolUse` is the snapshot before a shell command, a write, or a delete; `sessionStart` is where the briefing lands. Entries already in the file that run something else stay. A file that is not valid JSON is refused untouched. Running `ff hook cursor` on a wired file reports it as already wired and changes nothing.

## What `ff unhook cursor` removes

The two entries. `version` stays, because Cursor requires it and it was never fufu's.

```console
$ ff unhook cursor
cursor removed from ~/.cursor/hooks.json

$ cat ~/.cursor/hooks.json
{
  "version": 1
}
```

## Notes

`sessionStart` does not fire for cloud agents, so a cloud agent is never briefed. Capture still rides `preToolUse`, so the snapshots are there; what the agent lacks is the once-per-session spelling of fufu's verbs. `ff hook -l` and `ff doctor` say so whenever the hook is wired.
