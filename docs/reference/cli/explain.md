# ff explain

Look up a fufu error ID and read its meaning and recovery suggestions. Supply an ID from a refusal, or use `--list` to list known IDs. This works offline and outside a repository.

## Usage

```
Usage: ff explain [OPTIONS] [id]
```

## Examples

```sh
ff explain --list               # Find known error IDs
ff explain held/op-revert       # Read a refusal's explanation
ff explain held/op-revert --json  # Read it as structured fields
```

## Options

```
Arguments:
  [id]
          The error id to look up

Options:
      --list
          List every error id fufu knows

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Lookup behavior

Error IDs such as `held/op-revert` name refusals, not commits, changes, or operations. Copy the ID from the error output. An unknown ID is refused with lookup advice; no ID without `--list` is a usage error. If both are supplied, `--list` takes precedence.

The explanation describes the error and next commands. It does not retry the failed operation or change repository state.

The catalog lookup needs no network. Inside a repository, the CLI can still launch a passive update check and print a cached update notice; `fufu.updateCheck=false` disables those.
