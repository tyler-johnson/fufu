List configured remote names and their fetch URLs, one row per remote. Use these names when selecting a destination with `ff push --to`.

## Examples

```sh
ff remote                       # Names and fetch URLs
ff remote --json                # The same list as JSON
ff branch                       # Local and remote-only branches
```

### Options

### Managing remotes

Add or edit remote configuration through Git, for example `ff git remote add <name> <url>`. `ff pull` fetches from the current branch's selected remote; `ff push --to <name>` assigns a remote when the branch has none.

The list command changes no remote configuration. The CLI still attempts capture and can run maintenance. It does not fetch, and past-state flags are not supported.
