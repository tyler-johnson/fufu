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
ff show --stat HEAD             # File counts instead of a patch
ff show --no-patch HEAD~3       # Header and message only
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
      --stat
          Print the diffstat instead of the patch

      --name-only
          Print one path per line with its change type instead of the patch

      --no-patch
          Print the header and message only

      --json
          Emit machine-readable JSON

  -U, --unified <n>
          Context lines around each change; 3 when omitted

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

## Revisions and paths

The first argument is a [revision expression](../revisions.md#revision-names-and-suffixes) selecting exactly one member. `@` means the open change; `@^` means HEAD. A change-ID prefix needs four or more characters and a unique match; the open change's ID selects `@`. Subsequent paths select files or directory prefixes, without globs. They are checked against disk and HEAD, so a second revision in the path slot, such as `ff show HEAD <sha>`, is refused with `usage/no-such-path` rather than read as a filter that matches nothing.

Commit SHAs and operation IDs are both hexadecimal, but this position reads revisions. Use [`ff op show`](op-show.md) for operations. Blobs and trees use Git's syntax through [`ff git show`](git.md).

## Message

Show prints the full message: the subject, then a blank line and the body when present. The open change's pending description uses the same format. JSON carries `subject` and `body`; `body` is empty for a one-line message.

## Patches and signatures

Use `--stat` for per-file change counts, `--name-only` for paths and change types, or `--no-patch` for the header and message alone. These three flags are mutually exclusive. `-U <n>` sets the number of context lines in patches, 3 by default; it has no effect without a patch.

A commit is compared with its parent, or the empty tree for a root commit. A merge is compared with the auto-merge of its parents. A clean merge with no additional edits shows `(a clean merge: nothing beyond its parents)` and commands to compare it with each parent.

If the parents' auto-merge conflicts or has no common ancestor, show compares the merge with its first parent and explains the fallback in the header. That patch includes changes brought in from the other parents as well as the merge's own edits or resolution.

In JSON, `against` is `parent`, `auto-merge`, or `first-parent`. `--stat` omits each file's `hunks`; `--name-only` keeps `path`, `from`, `kind`, and `binary` per file and omits change counts. `--no-patch` omits `changes`, `insertions`, and `deletions`.

A signed commit is verified and receives a signature line with the verdict and signer. An unsigned commit has no signature line and runs no signer. JSON includes the result in `signature`.
