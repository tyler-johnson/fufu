# ff extension list

One row per declared extension — its name, the version recorded when it was declared, and the verbs it answers to — in the order they were declared, which is the order subscribers are fanned out in and the card names verbs in.

A row whose binary has left PATH says so and stays: dispatch is a fresh walk every time, so a name that resolves to nothing today is a fact about PATH rather than a reason to forget the declaration.

Two things can appear below the rows:

- A record claiming a contract this fufu does not speak. It is listed apart, and it is described to nobody.
- A registry file that does not read as one. That is a warning rather than a failure: the listing is empty, nothing on this machine is described while it reads that way, and [`ff doctor`](doctor.md) names the file.

## Usage

```
Usage: ff extension list [OPTIONS]

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
ff extension                 the same list, bare
ff extension list --json     the manifests as they were recorded
ff extension add tower       put a name on it
ff doctor                    every ff-<name> on PATH, declared or not
```
