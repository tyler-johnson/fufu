# ff done

Ends the editing session `ff edit` opened: the commit the session was opened on is amended with what the working tree now holds, what waited ahead is replayed onto it, and you land back on the branch the session left standing.

A replay that would conflict stops with nothing changed rather than leaving you mid-rewrite. `--abandon` drops the session instead of landing it, stashing whatever is uncommitted rather than discarding it.

It is one operation — the amend, the replay and the return move together — so one `ff undo` takes the whole session back.

## Usage

```
Usage: ff done [OPTIONS]

Options:
      --abandon
          Drop the session instead of landing it

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff done                        amend, replay what waited, land back
ff done --abandon              drop the session, stash what is open
```
