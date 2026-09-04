# ff extension add

Runs `ff-<name> --ff-manifest` and reads the one envelope it prints: the verbs the extension answers to, whether its writes are undoable, and the contract it speaks.

The flag is recognized before anything else on the command line and answers outside a repository, so fufu can ask a binary what it is before it has any reason to trust it. Nothing is handed down for the ask — an extension that needed the contract to state which contract it speaks would be echoing fufu's own number back.

Three checks stand between the answer and the record: the manifest parses as the machine surface types it, the contract it claims is the one this fufu speaks, and the name it gives is the name of the binary that was resolved. A manifest is refused whole rather than in part, because a half-declared extension is one fufu would describe and could not serve, and a refusal records nothing.

Declaring the same name again replaces the record and keeps its place in the order — upgrading a binary is not a reordering, and the order is what subscribers are fanned out in.

What is recorded is the manifest as it was read, unknown fields and all, plus the path the walk landed on and the time; [`ff doctor`](doctor.md) compares a binary against those to report drift. The path is evidence rather than a route: dispatch stays the PATH walk, so a binary that moves is still found.

## Usage

```
Usage: ff extension add [OPTIONS] <name>

Arguments:
  <name>
          The name after `ff-`, as the binary on PATH spells it

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
ff extension add tower       ask ff-tower what it is, and record it
ff extension list            what this machine declares now
ff hook claude               its skills and its server go in with fufu's
ff extension remove tower    take it back off
```
