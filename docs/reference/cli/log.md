# ff log

Show commit history, with the open change `@` above the recorded commits `●`. By default, show the last 25 rows from `HEAD`. The letters column contains change IDs, which [`ff evolog`](evolog.md) uses to show a change's recorded evolution.

## Usage

```
Usage: ff log [OPTIONS] [path]...
```

## Examples

```sh
ff log                          # Last 25 rows, including open work
ff log -n 0                     # Unlimited rows
ff log --commits                # Commit history without change IDs
ff log --signatures             # Verify signatures and show verdicts
ff log --body                   # Show full commit messages
ff log -p                       # Each row's patch under it
ff log --stat -n 5              # File counts under the last five rows
ff log -r main                  # Only main's tip
ff log -r 'trunk..@'            # Work beyond trunk, including @
ff log -r '@~3..@'              # Two commits and the open change
ff log src/parser.rs            # Follow a file through renames
ff log -r 'trunk..@' src/       # Filter a revision set by path
```

## Options

```
Arguments:
  [path]...
          Files or directories to limit the log to; all of them when omitted

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

  -r, --revisions <revset>
          Revisions to show, as a revset; without it, the walk from HEAD

      --commits
          Commits only — the plain history view

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --signatures
          Verify signatures and show verdicts; invokes external verifiers

      --body
          Print each row's message body under its subject

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

  -p, --patch
          Print each row's patch under it

      --stat
          Print the diffstat under each row instead of the patch

      --name-only
          Print one path per line with its change type under each row instead of the patch

      --session <name>
          Session name for this invocation

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -U, --unified <n>
          Context lines around each change; 3 when omitted

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

  -h, --help
          Print help (see a summary with '-h')
```

## Choosing rows

`-r` (`--revisions`) selects a revision set (revset) instead of walking from HEAD. It accepts commit SHAs, change IDs, branches, tags, suffixes, and set operators. `@` means the open change; `@^` is `HEAD`, and `@~3` is `HEAD~2`. The open row appears only if selected. See [Revisions and IDs](../revisions.md#revision-sets-and-grammar) for grammar and current set boundaries.

Positional arguments are paths, never revisions: `ff log main` filters the path main. Paths select files or directory prefixes, without globs. The open row appears only when it touches a selected path. A file is followed through renames by default; a directory is not. With `-r`, paths filter the selected commits without rename following.

`--commits` omits change IDs. `--body` shows full commit messages, including the open change's pending description. These two options cannot be combined. Without `--body`, rows show only the subject.

`--at` and `--at-op` are declared but currently refused for log. Use [`ff op log`](op-log.md) for recorded operations or [`ff history`](history.md) for undo steps.

## Patches under rows

Use `-p` (`--patch`) to print a patch below each row, `--stat` for per-file change counts, or `--name-only` for paths and change types. `--stat` and `--name-only` are mutually exclusive; either replaces the patch when combined with `-p`. None of these options combines with `--commits`.

`-U <n>` sets the number of context lines in patches, 3 by default. It has no effect without a patch.

Each commit is compared with its parent, and the open change with HEAD. Merges use the parents' auto-merge, falling back to the first parent if that auto-merge conflicts or has no common ancestor. This is the same comparison as [`ff show`](show.md).

## JSON output

Commit rows always include `body`. With `-p`, `--stat`, or `--name-only`, rows and the open block also include patch data. `--stat` omits each file's `hunks`; `--name-only` keeps `path`, `from`, `kind`, and `binary` per file and omits change counts.

Rows with patch data include `against`: `parent`, `auto-merge`, or `first-parent`. See [JSON output and scripting](../../agents/machine-surface.md) for the complete report shape.

## Change IDs and commit objects

A fufu commit stores a stable `change-id` header, also used by jj. Surviving changes keep it through fufu rewrites. A commit without the header derives its ID from its SHA, so an outside rewrite can change its identity.

The bold change-ID prefix is unique only on the displayed page. Resolution needs at least four characters and a unique repository match. Use more letters for an ambiguous prefix, or a commit SHA for divergent copies of one change. The [prefix reference](../revisions.md#prefixes-and-divergent-copies) describes lookup limits.

The `@` row's SHA identifies an internal open object, not a commit already recorded in branch history. A different message, partial commit, signing, or hook can change the SHA when [`ff commit`](commit.md) records it. The SHA is blank on a clean tree.

## Signatures and paging

`signed` means a signature is present, without verifying it. `--signatures` verifies signed commits and displays words such as `verified gpg 9B295D68`, `bad signature`, `untrusted key`, `expired signature`, `expired key`, `revoked key`, or `unverifiable`. Unsigned commits have no verdict. Verification uses up to eight workers; SSH normally invokes `ssh-keygen` twice per signed row, other formats once.

Terminal output uses `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. Piped output and JSON do not page.
