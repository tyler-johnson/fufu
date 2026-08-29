# ff lift

The other direction of absorb: takes paths out of a commit that has already closed and back into the open change — the revision you name, or the one it sits on when you name none. A lift does not attribute hunks either: whole files are what come back out, and a path filter only chooses which of the commit's files they are.

Everything above the target re-parents in the same operation, so a branch inside that range comes along with it. If the lift takes everything the commit held, the commit is dropped, because fufu writes no empty commit. What moves is the commit's identity and the stack above it; no file is copied or renamed in the re-point.

## Usage

```
Usage: ff lift [OPTIONS] [path]...

Arguments:
  [path]...
          Limit the lift to these paths (files or directory prefixes)

Options:
      --from <rev>
          Commit to lift out of; without it, the commit under the change

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
ff lift                        take everything out of the commit under it
ff lift --from HEAD~2          take it out of a commit further back
ff lift src/parser.rs          take only that path back out
```
