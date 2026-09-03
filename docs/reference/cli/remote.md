# ff remote

What the remotes here are called, and where each one points.

fufu's own verbs name a remote rather than assume one — ff publish --to takes a name, ff sync fetches from the one this branch answers to, and a refusal that could not tell which remote you meant sends you here. So the list those verbs are checked against is worth having inside fufu rather than borrowed from git. One row per remote, its fetch URL beside it.

A read and nothing more. Adding a remote is a name and a URL, two facts fufu has no verb for yet: [`ff git remote add <name> <url>`](git.md) is where that lives.

## Usage

```
Usage: ff remote [OPTIONS]

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
ff remote                      what the remotes here are called
ff remote --json               the same, for a machine
ff publish --to origin         send a branch to one of them, by name
ff branch list                 what those remotes are holding
```
