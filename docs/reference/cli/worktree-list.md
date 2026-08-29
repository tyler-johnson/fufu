# ff worktree list

The live worktrees first, with their checkouts and the branches they stand on, then the chains whose worktree is gone.

That second section is the earn: a chain lives in the shared ref namespace, so it outlives the checkout, and it is what keeps a deleted bay's work addressable. The tip op id on each row is what ff restore --at-op takes. Retention still ages those chains out on the ordinary fufu.keep cadence — surviving the worktree is not living forever.

## Usage

```
Usage: ff worktree list [OPTIONS]

Options:
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
ff worktree list                the worktrees, and the gone chains
ff worktree list --json         the same, for a machine
ff restore <path> --at-op <op>  bring a file back from one of them
ff trim                         age the gone chains out
```
