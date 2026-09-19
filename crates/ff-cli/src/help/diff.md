Show uncommitted work as a patch against the commit below the open change. With no paths, include the whole eligible change, including newly created untracked files.

## Examples

```sh
ff diff                         # Whole open change
ff diff src/                    # Changes under src/
ff diff --json                  # Hunks and lines as fields
ff diff > fix.patch             # Save a patch for git apply
ff status                       # File counts instead of a patch
ff op diff '@^' @               # Compare two recorded file trees
```

### Options

### Paths and output

Paths select files or directory prefixes, without globs. Snapshot exclusions apply, including ignored untracked files and content above `fufu.maxFileSize`. A positional that names no path on disk or in HEAD is refused with `usage/no-such-path`, so a revision such as `main..HEAD` in the path slot is an error rather than an empty patch; `ff show` reads one revision and `ff log -r` a set. A path that exists but has no changes prints an empty patch and exits 0.

Text output is a unified diff suitable for `git apply`. It does not repeat the diffstat from `ff status`. `ff show` adds the revision's identity and message to the patch; `ff commit` records eligible working changes in branch history.
