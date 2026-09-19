Show a patch: the open change by default, a set's total patch with `-r`, or the difference between two revisions with `--from` and `--to`. With no paths, include the whole eligible change, including newly created untracked files.

## Examples

```sh
ff diff                         # Whole open change
ff diff src/                    # Changes under src/
ff diff -r HEAD                 # What HEAD did
ff diff -r 'trunk..@'           # The branch plus open work, against trunk
ff diff --from main             # main to the open change
ff diff --from v1 --to v2       # Two points
ff diff --stat                  # File counts instead of a patch
ff diff --name-only             # Changed paths with kind letters
ff diff -U0 -r HEAD             # No context lines
ff diff --json                  # Hunks and lines as fields
ff diff > fix.patch             # Save a patch for git apply
ff op diff '@^' @               # Compare two recorded file trees
```

### Options

### Choosing what to compare

`-r` takes a revset and shows its total patch, measured from the root's parent to the head, so `-r @` is the default, `-r HEAD` is one commit, and `-r 'trunk..@'` is git's `main...HEAD` with the working copy included. A merge at the root is measured against the auto-merge of its parents as `ff show` measures it; when that falls back to the first parent, one `ff:` line on stderr says so. JSON under `-r` carries `against`: `parent`, `auto-merge`, or `first-parent`. A set with more than one head or root, or a gap, is refused with `usage/revset-not-a-range`; `ff log -r` shows the members.

`--from` and `--to` name two points, git's `git diff a b`. `--to` defaults to `@` and `--from` to `@^`, so `--from main` is what the branch plus open work changes against main. `@` reads the open change's tree. `-r` with either is refused with `usage/bad-flags`. Positional arguments are paths, never revisions; `ff show` reads one revision with its identity and message above the patch.

### Paths and output

Paths select files or directory prefixes, without globs. Snapshot exclusions apply, including ignored untracked files and content above `fufu.maxFileSize`. A positional that names no path on disk or in HEAD is refused with `usage/no-such-path`, so a revision such as `main..HEAD` in the path slot is an error rather than an empty patch; revisions go behind `-r`, `--from`, and `--to`. A path that exists but has no changes prints an empty patch and exits 0.

Text output is a unified diff suitable for `git apply`. `--stat` prints the diffstat block in place of the patch, and `--name-only` one path per line with its kind letter; the two do not combine and are refused together with `usage/bad-flags`. `-U <n>` sets the context lines around each change, 3 by default; without a patch it is accepted and changes nothing. In JSON, `--stat` drops each file's `hunks`, and `--name-only` keeps `path`, `from`, `kind`, and `binary` and drops the counts. `ff show` adds the revision's identity and message to the patch; `ff commit` records eligible working changes in branch history.
