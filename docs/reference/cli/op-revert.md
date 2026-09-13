# ff op revert

Invert one operation's recorded ref transitions while keeping current files and the index. Every affected ref must still have the value that operation left. If later work moved any of them, revert refuses rather than replaying around that work.

## Usage

```
Usage: ff op revert [OPTIONS] <op>
```

## Examples

Choose an operation ID from the log, then substitute it for OP_ID below.

```sh
ff op log 'kind(op)'            # Find an operation with ref changes
ff op show OP_ID                # Inspect those transitions first
ff op revert OP_ID              # Invert them if they still apply
ff undo                         # Undo the successful revert
```

## Options

```
Arguments:
  <op>
          The operation to invert

Options:
      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Applicability and recovery

Only command operations and recorded outside changes with ref transitions can be reverted. Captures and notes have nothing to invert. Use [`ff restore --all --at-op <id>`](restore.md) for files from a capture, or [`ff op restore <id>`](op-restore.md) to restore recorded local state.

A ref that moved later produces `held/op-revert` (exit 3). No inverse transitions are applied, but pre-command capture and reconciliation can already have written records. This refusal does not create a rewrite-resolution session for [`ff resolve`](resolve.md); inspect the named refs and choose an applicable operation or another recovery command.

## What a successful revert records

The inverse transitions are appended as a new operation, so [`ff undo`](undo.md) can reverse the revert. The HEAD selection, working-copy files, and index stay in place, although reversing the current branch's tip changes the commit those files are compared against. Remote effects are not reversed.
