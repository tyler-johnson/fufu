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
ff diff --name-only             # Changed paths and change types
ff diff -U0 -r HEAD             # No context lines
ff diff --json                  # Hunks and lines as fields
ff diff > fix.patch             # Save a patch for git apply
ff op diff '@^' @               # Compare two recorded file trees
```

### Options

### Choosing what to compare

`-r` selects a revision set and shows its combined patch. `-r HEAD` shows the latest commit; `-r 'trunk..@'` includes the branch's commits and open work beyond trunk. The set must be connected, with one root and one head. Use `ff log -r <revset>` to inspect its members if it is refused with `usage/revset-not-a-range`. The open change alone, `-r @`, is accepted.

The patch runs from the root's parent to the head. A merge root uses the auto-merge of its parents, falling back to the first parent when that auto-merge conflicts or has no common ancestor. A fallback is reported on stderr. A root commit with no parents uses the empty tree.

`--from` and `--to` compare two revisions, like `git diff a b`. `--to` defaults to `@`, the open change; when only `--to` is supplied, `--from` defaults to HEAD. Thus `--from main` compares main with the branch's current files, including open work. Use these flags together or use `-r`; combining the two forms is refused with `usage/bad-flags`.

### Paths and output

Paths select files or directory prefixes, without globs. Snapshot exclusions apply, including ignored untracked files and content above `fufu.maxFileSize`. A positional that names no path on disk or in HEAD is refused with `usage/no-such-path`, so a revision such as `main..HEAD` in the path slot is an error rather than an empty patch; revisions go behind `-r`, `--from`, and `--to`. A path that exists but has no changes prints an empty patch and exits 0.

Text output is a unified diff suitable for `git apply`. Use `--stat` for per-file change counts or `--name-only` for paths and change types. These two flags are mutually exclusive. `-U <n>` sets the number of context lines, 3 by default; it has no effect when no patch is printed.

### JSON output

`--stat` omits each file's `hunks`. `--name-only` keeps `path`, `from`, `kind`, and `binary` per file and omits change counts. With `-r`, `against` identifies the comparison base: `parent`, `auto-merge`, or `first-parent`. Bare diff and two-point comparisons omit `against`. See [JSON output and scripting](../../agents/machine-surface.md).
