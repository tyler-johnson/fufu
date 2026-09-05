# ff resolve

A held rewrite is a conflict fufu chose not to interrupt you with — and this is where you choose to deal with it, all at once. Every surviving conflict region lands in the working copy together, as ordinary labeled markers, in one session: fix them, then [`ff done`](done.md) lands the rewrite.

Nothing moves. Your branch does not move and the parked change, if there is one, waits where it was — the session is recorded in the branch's own metadata, and the hold stays, because it is what the session is resolving. If the world has moved and the rewrite applies cleanly now, the hold is released instead, and re-running the verb that recorded it lands it.

--abandon drops the hold — and an open session's markers with it — so it is also the way out of one. The way back, either way, is one ff undo.

## Usage

```
Usage: ff resolve [OPTIONS]

Options:
      --abandon
          Drop the pending rewrite instead of resolving it

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
ff resolve                   materialize the hold's conflicts and fix them
ff done                      land the fixes, and the rewrite behind them
ff resolve --abandon         drop the hold instead
ff undo                      take the session back, markers and all
```
