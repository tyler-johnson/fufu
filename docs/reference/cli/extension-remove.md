# ff extension remove

Takes the name off the list. fufu stops describing the extension — its verbs leave the tool and the card, `ff help <name>` stops reaching the binary, its briefing line and its skills and its subscriptions all stop being fufu's business.

Nothing is uninstalled. `ff-<name>` is still on PATH and `ff <name>` still runs it, on the same three variables it always had. Skills a [`ff hook`](hook.md) install already wrote stay where they were written; the next install is what stops carrying them.

A name that was never declared is refused rather than answered as done. [`ff undo`](undo.md) does not reach any of this either: the list is per machine and lives outside every repository, so the way back is [`ff extension add <name>`](extension-add.md) again.

## Usage

```
Usage: ff extension remove [OPTIONS] <name>

Arguments:
  <name>
          The name it was declared under

Options:
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
ff extension remove tower    fufu stops describing it
ff extension list            what is left
ff extension add tower       declare it again
```
