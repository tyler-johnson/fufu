# ff op diff

Compare the recorded file trees of two operations. The first address is required; the second defaults to `@`, the live operation tip. By default, print a diffstat; `-p` adds the patch.

## Usage

```
Usage: ff op diff [OPTIONS] <a> [b]
```

## Examples

```sh
ff op diff '@^' @               # Files across the newest operation
ff op diff '@~3'                # From three operations ago to now
ff op diff -p '@~3' '@^'        # Compare two older states with a patch
```

## Options

```
Arguments:
  <a>
          The older operation

  [b]
          The newer operation; `@` when omitted

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

## Addresses and comparison scope

Addresses are hexadecimal operation IDs or unique prefixes, or `@` with predecessor suffixes. `@^` means the previous operation. See [operation addresses](../revisions.md#operation-expressions).

`--at-op` or `--at` supplies the second address when that positional argument is omitted. An explicit second address takes precedence. The CLI attempts a snapshot before reading, so the live tip can include current edits.

This compares files, not ref transitions. Use [`ff op show`](op-show.md) to inspect ref movement. Adjacent operations can be on different branches; comparing across a switch can show the entire checkout difference. [`ff diff`](diff.md) shows only the current uncommitted change.
