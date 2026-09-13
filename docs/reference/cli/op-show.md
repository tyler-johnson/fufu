# ff op show

Show one recorded operation: what ran, when, on which branch, its ref transitions, and a file diffstat against its predecessor. With no address or past-state flag, show `@`, the live operation tip.

## Usage

```
Usage: ff op show [OPTIONS] [op]
```

## Examples

```sh
ff op show                      # Live operation tip
ff op show '@^'                 # Its predecessor
ff op show -p @                 # Include the file patch
ff op show --at 2h              # Operation current two hours ago
ff op show --json               # The same record as fields
```

## Options

```
Arguments:
  [op]
          The operation; `@` (the newest) when omitted

Options:
  -p, --patch
          Print the patch under the diffstat, not just the counts

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

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

## Address and output

The address is a hexadecimal operation ID or unique prefix, or `@` with predecessor suffixes such as `@~3`. It is not a commit SHA or change ID; those belong to [`ff show`](show.md). See [operation addresses](../revisions.md#operation-expressions).

`--at-op` or `--at` supplies the address only when the positional address is omitted. An explicit address takes precedence. The CLI attempts capture first, so `@` can name this invocation's new snapshot.

Every operation holds a file tree. `-p` adds its patch below the diffstat. Ref transitions are listed separately; a capture has no ref transitions. Adjacent operations can describe different branches, so their file difference can include a checkout change.
