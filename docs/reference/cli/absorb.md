# ff absorb

Folds the open change into a commit that has already closed — the revision you name, or the one it sits on when you name none. An absorb does not attribute hunks: the change is the unit, and a path filter only chooses which of its files fold in, leaving the rest open.

Everything above the target re-parents in the same operation, so a branch inside that range comes along with it. What moves is the commit's identity and the stack above it; no file is copied or renamed in the re-point.

## Usage

```
Usage: ff absorb [OPTIONS] [path]...

Arguments:
  [path]...
          Limit the absorb to these paths (files or directory prefixes)

Options:
      --into <rev>
          Commit to absorb into; without it, the commit under the change

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
ff absorb                      fold everything open into the commit under it
ff absorb --into HEAD~2        fold it into a commit further back
ff absorb src/parser.rs        fold only that path
```
