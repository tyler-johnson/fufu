# ff version

Print the fufu release, build commit and date when available, and project URL. If the update cache knows a newer release, also print its availability. `ff -v` gives the same result.

## Usage

```
Usage: ff version [OPTIONS]
```

## Examples

```sh
ff version                      # Release and build identity
ff -v                           # Same result as a flag
ff version --json               # Version and update status as fields
```

## Options

```
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

## Build and update information

A build made without Git metadata, such as a source archive, prints the release without a commit or date. JSON uses null for missing build fields and includes the cached update status.

The displayed update result reads the cache without waiting for the network. In a repository, the CLI's passive update check can refresh that cache in the background when enabled. [`ff update`](update.md) reports the update command appropriate for this installation.
