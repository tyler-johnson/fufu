Print the fufu release, build commit and date when available, and project URL. If the update cache knows a newer release, also print its availability. `ff -v` gives the same result.

## Examples

```sh
ff version                      # Release and build identity
ff -v                           # Same result as a flag
ff version --json               # Version and update status as fields
```

### Options

### Build and update information

A build made without Git metadata, such as a source archive, prints the release without a commit or date. JSON uses null for missing build fields and includes the cached update status.

The displayed update result reads the cache without waiting for the network. In a repository, the CLI's passive update check can refresh that cache in the background when enabled. `ff update` reports the update command appropriate for this installation.
