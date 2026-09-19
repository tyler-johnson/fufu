Show commit history, with the open change `@` above the recorded commits `●`. By default, show the last 25 rows from `HEAD`. The letters column contains change IDs, which `ff evolog` uses to show a change's recorded evolution.

## Examples

```sh
ff log                          # Last 25 rows, including open work
ff log -n 0                     # Unlimited rows
ff log --commits                # Commit history without change IDs
ff log --signatures             # Verify signatures and show verdicts
ff log --body                   # Messages whole, bodies under subjects
ff log -p                       # Each row's patch under it
ff log --stat -n 5              # File counts under the last five rows
ff log -r main                  # Only main's tip
ff log -r 'trunk..@'            # Work beyond trunk, including @
ff log -r '@~3..@'              # Two commits and the open change
ff log src/parser.rs            # Follow a file through renames
ff log -r 'trunk..@' src/       # Filter a revision set by path
```

### Options

### Choosing rows

`-r` (`--revisions`) selects a revision set (revset) instead of walking from HEAD. It accepts commit SHAs, change IDs, branches, tags, suffixes, and set operators. `@` means the open change; `@^` is `HEAD`, and `@~3` is `HEAD~2`. The open row appears only if selected. See [Revisions and IDs](../revisions.md#revision-sets-and-grammar) for grammar and current set boundaries.

Positional arguments are paths, never revisions: `ff log main` filters the path main. Paths select files or directory prefixes, without globs. The open row appears only when it touches a selected path. A file is followed through renames by default; a directory is not. With `-r`, paths filter the selected commits without rename following.

`--commits` omits change IDs. `--body` prints each row's body under its subject, the open row's pending description included, and does not combine with `--commits`. Rows stay compact without it; JSON rows carry `body` always. `--at` and `--at-op` are declared but currently refused for log. `ff op log` lists recorded operations, and `ff history` lists undo steps.

### Patches under rows

`-p` (`--patch`) prints each row's patch under it, measured against the commit's first parent as `ff show` measures it, and the open row's against HEAD. A merge row carries none. `--stat` and `--name-only` are the shorter forms, the diffstat block and one path per line with its kind letter, and `--stat` outranks `-p`. None of the three combine with `--commits`. `-U <n>` sets the context lines around each change, 3 by default, and changes nothing without a patch. In JSON, rows and the open block gain `changes`, `insertions`, and `deletions` under any of the three, with the same drops as `ff diff`: `hunks` under `--stat`, the counts under `--name-only`. A merge row's three are null.

### Change IDs and commit objects

A fufu commit stores a stable `change-id` header, also used by jj. Surviving changes keep it through fufu rewrites. A commit without the header derives its ID from its SHA, so an outside rewrite can change its identity.

The bold change-ID prefix is unique only on the displayed page. Resolution needs at least four characters and a unique repository match. Use more letters for an ambiguous prefix, or a commit SHA for divergent copies of one change. The [prefix reference](../revisions.md#prefixes-and-divergent-copies) describes lookup limits.

The `@` row's SHA identifies an internal open object, not a commit already recorded in branch history. A different message, partial commit, signing, or hook can change the SHA when `ff commit` records it. The SHA is blank on a clean tree.

### Signatures and paging

`signed` means a signature is present, without verifying it. `--signatures` verifies signed commits and displays words such as `verified gpg 9B295D68`, `bad signature`, `untrusted key`, `expired signature`, `expired key`, `revoked key`, or `unverifiable`. Unsigned commits have no verdict. Verification uses up to eight workers; SSH normally invokes `ssh-keygen` twice per signed row, other formats once.

Terminal output uses `fufu.pager`, then `FF_PAGER`, then `PAGER`, then `less`. Piped output and JSON do not page.
