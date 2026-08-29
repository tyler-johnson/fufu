# ff evolog

Every operation on the change you have open, newest first — the drill-in behind the letters column in `ff log`. This is where a lost hour is found: each row is a whole worktree, and `ff restore --at-op <id>` brings any of them back.

Because fufu captures before it works, the newest row is often this command's own capture, taken a moment ago when it found the tree dirty. That is intended.

Ids are spelled in the letters k–z, never hex digits, so an operation id can never be misread as a commit sha. The bold prefix is the shortest one `ff op` and `--at-op` resolve unambiguously.

-p prints each row's patch under it — what that one operation changed, measured against the capture before it on this branch.

## Usage

```
Usage: ff evolog [OPTIONS]

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

  -p, --patch
          Print each row's patch under it — what that operation changed

      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

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
ff evolog                      the open change's operations
ff evolog -n 0                 all of them
ff evolog -p                   each row with what it changed, in full
ff restore src/ --at-op <id>   pull a directory back from one
```
