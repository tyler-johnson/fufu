Show one revision's identity, author, age, message, and patch. With no revision, show `@`, the open change, with the same patch as `ff diff`.

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

### Options

### Revisions and paths

The first argument is a [revision expression](../revisions.md#revision-names-and-suffixes) selecting exactly one member. `@` means the open change; `@^` means HEAD. A change-ID prefix needs four or more characters and a unique match; the open change's ID selects `@`. Subsequent paths select files or directory prefixes, without globs. They are checked against disk and HEAD, so a second revision in the path slot, such as `ff show HEAD <sha>`, is refused with `usage/no-such-path` rather than read as a filter that matches nothing.

Commit SHAs and operation IDs are both hexadecimal, but this position reads revisions. Use `ff op show` for operations. Blobs and trees use Git's syntax through `ff git show`.

### Message

The message prints whole: the subject, then a blank line and the body when there is one, each line indented two spaces. The open change's pending description prints the same way. JSON carries `subject` and `body`; `body` is empty for a one-line message.

### Patches and signatures

A commit's patch is measured against its parent. A merge is measured against the auto-merge of its parents, so a clean merge shows `(a clean merge: nothing beyond its parents)` and the two `ff diff --from` commands for the per-parent view, and a merge that resolved a conflict or carried an edit of its own shows that. Parents with no merge base, or an auto-merge that conflicts, are measured against the first parent, and the header says so. JSON carries `against`: `parent`, `auto-merge`, or `first-parent`.

`--stat`, `--name-only`, and `--no-patch` shorten the patch to the diffstat, the paths with their kind letters, or nothing, one at a time; two together are refused with `usage/bad-flags`. `-U <n>` sets the context lines around each change, 3 by default, and changes nothing without a patch. In JSON, `--stat` drops each file's `hunks`, `--name-only` keeps `path`, `from`, `kind`, and `binary` per file and drops the counts, and `--no-patch` drops `changes`, `insertions`, and `deletions`.

A signed commit is verified and receives a signature line with the verdict and signer. An unsigned commit has no signature line and runs no signer. JSON includes the result in `signature`.
