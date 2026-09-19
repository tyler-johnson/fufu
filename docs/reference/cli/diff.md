# ff diff

Show a patch: the open change by default, a set's total patch with `-r`, or the difference between two revisions with `--from` and `--to`. With no paths, include the whole eligible change, including newly created untracked files.

## Usage

```
Usage: ff diff [OPTIONS] [path]...
```

## Examples

```sh
ff diff                         # Whole open change
ff diff src/                    # Changes under src/
ff diff -r HEAD                 # What HEAD did
ff diff -r 'trunk..@'           # The branch plus open work, against trunk
ff diff --from main             # main to the open change
ff diff --from v1 --to v2       # Two points
ff diff --json                  # Hunks and lines as fields
ff diff > fix.patch             # Save a patch for git apply
ff status                       # File counts instead of a patch
ff op diff '@^' @               # Compare two recorded file trees
```

## Options

```
Arguments:
  [path]...
          Files or directories to limit the patch to; all of them when omitted

Options:
  -r, --revisions <revset>
          Revisions whose total patch to show, as a revset; the open change without it

      --from <rev>
          Older end of a two-point patch; `@^`, HEAD, when only --to is given

      --to <rev>
          Newer end of a two-point patch; `@`, the open change, when omitted

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

## Choosing what to compare

`-r` takes a revset and shows its total patch, measured from the root's first parent to the head, so `-r @` is the default, `-r HEAD` is one commit, and `-r 'trunk..@'` is git's `main...HEAD` with the working copy included. A set with more than one head or root, a gap, or a merge at its root is refused with `usage/revset-not-a-range`; [`ff log -r`](log.md) shows the members.

`--from` and `--to` name two points, git's `git diff a b`. `--to` defaults to `@` and `--from` to `@^`, so `--from main` is what the branch plus open work changes against main. `@` reads the open change's tree. `-r` with either is refused with `usage/bad-flags`. Positional arguments are paths, never revisions; [`ff show`](show.md) reads one revision with its identity and message above the patch.

## Paths and output

Paths select files or directory prefixes, without globs. Snapshot exclusions apply, including ignored untracked files and content above `fufu.maxFileSize`. A positional that names no path on disk or in HEAD is refused with `usage/no-such-path`, so a revision such as `main..HEAD` in the path slot is an error rather than an empty patch; revisions go behind `-r`, `--from`, and `--to`. A path that exists but has no changes prints an empty patch and exits 0.

Text output is a unified diff suitable for `git apply`. It does not repeat the diffstat from [`ff status`](status.md). `ff show` adds the revision's identity and message to the patch; [`ff commit`](commit.md) records eligible working changes in branch history.
