# ff history

Where you can go back to. `ff op log` answers what happened; this answers the other question, and they are not the same question — captures outnumber verb operations by more than an order of magnitude, so a log is mostly a machine's account of itself.

One row is one keystroke. `@` is where the repository stands; each row below it is one more press of `ff undo`, and each row above is one more press of `ff redo`. A run of captures collapses into the single row it undoes as, and says how many it collapsed — a keystroke that moved forty operations should not have to be inferred.

The redo path is whatever is still reversible. Landing work after an undo forks the log rather than truncating it, so the rows above `@` simply stop being offered once that happens.

Ids are the ones the `ff op` verbs take, so any row is also `ff op show <id>` and `ff op restore <id>`.

## Usage

```
Usage: ff history [OPTIONS]

Options:
  -n, --max-count <COUNT>
          Number of undo steps to show; 0 means unlimited
          
          [default: 25]

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
ff history                     the last 25 undo steps
ff history -n 0                every step back to the floor
ff history --json              the same, for machines
ff op show <id>                what one of those rows was
```
