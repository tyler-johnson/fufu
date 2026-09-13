# ff absorb

Add your uncommitted changes to an existing commit. By default, update the latest commit on the current branch. Use `--into` to update an earlier commit. `ff squash` is an alias.

## Usage

```
Usage: ff absorb [OPTIONS] [path]...
```

## Examples

```sh
ff absorb                       # Add all eligible uncommitted changes
ff absorb src/parser.rs         # Add only this file's changes
ff absorb --into HEAD~2         # Update an earlier commit
ff absorb --from 'HEAD~2..HEAD'  # Combine two commits into their parent
ff absorb -m "parser: handle escapes"  # Also change the target's message
```

## Options

```
Arguments:
  [path]...
          Limit the move to these paths (files or directory prefixes)

Options:
      --from <revset>
          Source revision set; defaults to the open change

      --into <rev>
          Target revision; defaults to the commit below the sources

  -m <msg>
          New target message, or pending description for an open target

      --json
          Emit machine-readable JSON

      --no-verify
          Skip pre-commit and commit-msg hooks

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

## Paths, messages, and ranges

Paths select files or directory prefixes, without globs or hunk selection. Unselected changes stay where they are. `-m` changes the target's message; an open target receives a pending description. Without it, the target keeps its message.

`--from <revset>` selects a contiguous run on the branch's history, optionally including the open change `@`. With no `--into`, absorb targets the commit immediately below the sources. When committed sources would default to a target on trunk's history, the move is refused: name the intended target explicitly. This guard does not forbid an explicit trunk target or adding only uncommitted work to trunk's tip.

`ff absorb` and [`ff lift`](lift.md) share the same move engine with different defaults. An explicit target can be below, above, or inside the source run, or `@`. A source emptied by the move is dropped. Surviving changes keep their change IDs while commit hashes change.

Preview a range with [`ff log -r 'HEAD~2..HEAD'`](log.md). An omitted right endpoint can include other branches. See [Revisions and IDs](../revisions.md#revision-sets-and-grammar) for the full syntax.

## Conflicts and dependent branches

Commits between and above the endpoints replay in the same operation, including branches inside that range. A conflicting primary replay records a held rewrite without landing it and exits 3; captures and metadata may still be written. [`ff resolve`](resolve.md) opens the conflicts.

After a successful move, dependent local branches replay parent before child. A downstream conflict holds that branch and leaves its dependents alone; the original move still lands and currently exits 0. Inspect the cascade report or [`ff status`](status.md). Branches checked out elsewhere, already held, or containing merges are skipped and named. One [`ff undo`](undo.md) takes back the move and its cascade.

## Hooks

When sources include the open change, `pre-commit` runs with the selected content in the index. Moves between recorded commits do not run it. `-m` on a recorded target runs message hooks as a reword does. A failing hook refuses the move; `--no-verify` skips `pre-commit` and `commit-msg`.
