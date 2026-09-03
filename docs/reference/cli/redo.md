# ff redo

The complement of [`ff undo`](undo.md): step forward again along the branch of the log an undo stepped off. Takes no argument, and repeats — each one goes one run further forward, until the log is back where it started.

Redo reads where the operation ref has been, so it can only follow a path that is still there. New work after an undo forks the log rather than truncating it: nothing is discarded, but redo stops offering a way forward it can no longer take, and says so. The forked-off branch keeps its ids, and [`ff op restore`](op-restore.md) still lands on any of them until trim ages them out.

## Usage

```
Usage: ff redo [OPTIONS]

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
ff undo && ff redo             back, and forward again
ff redo                        …and again, after several undos
ff op log                      where the log stands now
```
