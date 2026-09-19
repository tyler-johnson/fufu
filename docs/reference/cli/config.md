# ff config

Read or change fufu settings. With no arguments, list every setting, its value, meaning, and default marker. A key reads one setting; a key and value set it for this repository. `ff cfg` is the short spelling.

## Usage

```
Usage: ff config [OPTIONS] [key] [value]
```

## Examples

```sh
ff config                       # List settings
ff config keep                  # Read snapshot retention
ff config keep 30d              # Set retention in this repository
ff config --global pager bat    # Set the user-level pager
ff config gitPolicy strict      # Refuse covered Git passthrough commands
ff config --unset autoTrim      # Remove the repository override
```

## Options

```
Arguments:
  [key]
          Setting name — case-insensitive, the fufu. prefix optional

  [value]
          New value to set for this repo (--global: every repo)

Options:
      --unset
          Remove this scope's value, exposing inherited values or the default

      --global
          Apply the set/unset to every repo (user-level git config)

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

## Scope and validation

`--global` writes user-level Git configuration for all repositories. `--unset` removes the selected scope's value, exposing a lower-precedence value or the default. Setting names are case-insensitive and the `fufu.` prefix is optional.

Values are validated before writing. Storage and precedence use ordinary Git configuration under `fufu.<key>`. A malformed value written outside this command can be ignored by its reader in favor of a default; [`ff doctor`](doctor.md) reports invalid settings.
