# ff git

Run a Git command through fufu's snapshot and Git-policy checks. Attempts a snapshot before a permitted command; a strict-policy refusal happens before capture. If capture fails, fufu warns and Git still runs. Read this page with `ff help git`: `ff git --help` is Git's help.

## Usage

```
Usage: ff git [OPTIONS] [ARGS]...
```

## Examples

```sh
ff git status                   # Snapshot, then Git status
ff git tag                      # List tags through Git
ff config gitPolicy strict      # Refuse covered Git commands
ff config gitPolicy observe     # Record policy use without advice
ff help git                     # Fufu's passthrough help
```

## Options

```
Arguments:
  [ARGS]...
          Arguments passed to git verbatim

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
```

## Arguments and policy

All arguments after `git` pass to Git verbatim, including flags such as `--help`. Git controls its output; `--json` does not wrap it in fufu's envelope.

`fufu.gitPolicy` governs commands for which fufu has an alternative:

- `observe` records policy use and stays quiet.
- `coach` (default) adds advice naming the fufu command, once per word.
- `strict` refuses the covered command and names the alternative. It still records the policy tally.

Commands with no covered alternative, such as apply, bisect, and gc, run in every mode. Tag and merge passthroughs also run in every mode. Git commit and rebase are refused under strict policy.

## Shell and agent integration

The active shell alias `alias git='ff git'`, installed by [`ff hook <shell>`](hook.md), routes typed Git commands here. Activate that configuration as instructed; editing an rc file alone does not change the current shell.

Agent hooks use the same policy for received raw Git calls. They attempt capture before policy evaluation. Coach supplies context, while strict asks the client to deny the action through its protocol; client enforcement is separate from a passthrough refusal.
