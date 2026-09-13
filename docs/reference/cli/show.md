# ff show

Show one revision's identity, author, age, message, and patch. With no revision, show `@`, the open change, with the same patch as [`ff diff`](diff.md).

## Usage

```
Usage: ff show [OPTIONS] [rev] [path]...
```

## Examples

```sh
ff show                         # Open change with its header
ff show HEAD                    # Latest recorded commit
ff show HEAD~2 src/             # An earlier commit, filtered by path
ff show --json                  # Header and patch as fields
ff git show HEAD:file.txt       # Read a blob through Git
```

## Options

```
Arguments:
  [rev]
          The revision; `@`, the open change, when omitted

  [path]...
          Files or directories to limit the patch to; all of them when omitted

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

## Revisions and paths

The first argument is a [revision expression](../revisions.md#revision-names-and-suffixes) selecting exactly one member. `@` means the open change; `@^` means HEAD. A change-ID prefix needs four or more characters and a unique match; the open change's ID selects `@`. Subsequent paths select files or directory prefixes, without globs.

Commit SHAs and operation IDs are both hexadecimal, but this position reads revisions. Use [`ff op show`](op-show.md) for operations. Blobs and trees use Git's syntax through [`ff git show`](git.md).

## Patches and signatures

A non-merge commit's patch is measured against its first parent. A merge reports why no single patch is shown and points to Git's per-parent view.

A signed commit is verified and receives a signature line with the verdict and signer. An unsigned commit has no signature line and runs no signer. JSON includes the result in `signature`.
