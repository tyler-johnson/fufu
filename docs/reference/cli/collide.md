# ff collide

Check whether two branches' file changes would conflict when combined. With one branch name, compare it with the current branch. With two names, compare those branches.

## Usage

```
Usage: ff collide [OPTIONS] <branch>...
```

## Examples

```sh
ff collide feat-x               # Compare with the current branch
ff collide feat-x feat-y        # Compare two named branches
ff collide feat-x --json        # Read the verdict in a script
```

## Options

```
Arguments:
  <branch>...
          Branches to compare; one name compares with the current branch

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

## Scope and side effects

The comparison performs a three-way merge in memory without changing branch tips, the index, or worktree files. The CLI can still capture, auto-fetch, and run maintenance around it. `--no-fetch` skips fetching.

Each side uses the tree recorded for that branch, so saved uncommitted work counts even when a branch is checked out elsewhere or nowhere. A `*` beside the branch name marks use of that work. Unsaved or uncaptured edits in another worktree are not available to the comparison.

A collision is a finding: both clean and conflicting comparisons exit 0. Scripts must inspect the JSON verdict. This checks the two branches against each other; it does not pull, restack, or record a held rewrite.
