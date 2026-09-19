# ff remote

List configured remote names and their fetch URLs, one row per remote. Use these names when selecting a destination with [`ff push --to`](push.md).

## Usage

```
Usage: ff remote [OPTIONS]
```

## Examples

```sh
ff remote                       # Names and fetch URLs
ff remote --json                # The same list as JSON
ff branch                       # Local and remote-only branches
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

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Managing remotes

Add or edit remote configuration through Git, for example [`ff git remote add <name> <url>`](git.md). [`ff pull`](pull.md) fetches from the current branch's selected remote; `ff push --to <name>` assigns a remote when the branch has none.

The list command changes no remote configuration. The CLI still attempts capture and can run maintenance. It does not fetch, and past-state flags are not supported.
