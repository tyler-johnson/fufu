# ff explain

Look up an error id and see what it means.

## Usage

```
Usage: ff explain [OPTIONS] [id]

Arguments:
  [id]
          The error id to look up

Options:
      --list
          List every error id fufu knows

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help
```
