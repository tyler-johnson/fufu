# ff undo

Whole-repo undo: refs and the working tree together, not one without the other. It takes no argument and repeats — each one goes one step further back.

A step is a *run*, not an operation. Captures happen at machine rate and a person's undo does not, so undo steps over the longest stretch of adjacent captures carrying the same session, ending at the first operation that is not one.

A verb's operation is a decision somebody made, so it is always its own step — a switch and a commit are two undos, never one — which is also what keeps undo from rolling past a commit by accident.

Undo moves the log's pointer rather than appending, so the log records work and never navigation, and [`ff redo`](redo.md) is what comes forward again. Nothing is discarded: what an undo steps off stays reachable, with the capture taken just before it at the head, so redo hands back the work you were holding first.

Naming one operation instead of a run is [`ff op restore <op>`](op-restore.md).

## Usage

```
Usage: ff undo [OPTIONS]

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
ff undo                        step back one run of work
ff undo                        …and again, further back
ff redo                        forward again
ff op log                      what the log holds, with ids
ff op restore kqzm             land on one named operation instead
```
