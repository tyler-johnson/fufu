Compare the recorded file trees of two operations. The first address is required; the second defaults to `@`, the live operation tip. By default, print a diffstat; `-p` adds the patch.

## Examples

```sh
ff op diff '@^' @               # Files across the newest operation
ff op diff '@~3'                # From three operations ago to now
ff op diff -p '@~3' '@^'        # Compare two older states with a patch
```

### Options

### Addresses and comparison scope

Addresses are hexadecimal operation IDs or unique prefixes, or `@` with predecessor suffixes. `@^` means the previous operation. See [operation addresses](../revisions.md#operation-expressions).

`--at-op` or `--at` supplies the second address when that positional argument is omitted. An explicit second address takes precedence. The CLI attempts a snapshot before reading, so the live tip can include current edits.

This compares files, not ref transitions. Use `ff op show` to inspect ref movement. Adjacent operations can be on different branches; comparing across a switch can show the entire checkout difference. `ff diff` shows only the current uncommitted change.
