# ff op revert

Invert one operation and leave everything after it standing. Where `ff op restore` rewinds to a moment, this undoes a single change in the middle of later work.

It is the one verb in this family that *writes* an operation, because inverting one change while later work stands is itself a thing that happened, and the log should say so.

An inversion that no longer applies cleanly holds rather than guessing: the conflict is reported and nothing is written.

## Usage

```
Usage: ff op revert [OPTIONS] <op>

Arguments:
  <op>
          The operation to invert

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
ff op revert kqzm              take that one change back out
ff op log                      …and see the revert recorded
ff undo                        take the revert back too
```
