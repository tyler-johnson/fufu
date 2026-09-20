# ff fold

Replay the current branch onto a target, advance the target, and delete the source branch. With no target, use trunk. This worktree switches to the target with its uncommitted work still open. One [`ff undo`](undo.md) reverses the local operation.

## Usage

```
Usage: ff fold [OPTIONS] [branch]
```

## Examples

```sh
ff fold                         # Fold this branch into trunk
ff fold release-1.2             # Fold into another local branch
ff fold --stay                  # Keep this branch and checkout
ff undo                         # Restore the branch and target
```

## Options

```
Arguments:
  [branch]
          Branch to fold into; without it, trunk

Options:
      --stay
          Keep this branch and checkout; allow a target checked out elsewhere

      --json
          Emit machine-readable JSON

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

## Target and replay rules

Fold replays commits and advances the target without adding a merge commit. The source's timeline pointer moves to trash and its tip remains pinned by the operation. If the source is already on or ahead of the target's tip, no replay is needed.

Trunk cannot be folded, a branch cannot target itself, and the target must be local. A target checked out elsewhere requires `--stay`.

## Conflicts and dependent branches

The source's commits replay from their fork point with the target, followed by its open change. A conflicting primary replay refuses to land. Use [`ff restack --onto <target>`](restack.md) to record that replay as a held rewrite, then [`ff resolve`](resolve.md) and finish it before folding again. Pre-operation capture can still occur. A held restack on the source is dropped with the fold and named; a held absorb, lift, or done refuses the fold.

Dependents replay parent before child and record the target as their new base. A conflicting dependent holds in place, leaving branches above it alone. Branches checked out elsewhere or already held are skipped and named. The exit is 3 if a dependent holds. JSON reports these results in `cascade`.

## Keeping the source with --stay

`--stay` keeps the source branch and this checkout, moves the source to the new tip, and records the target as its base. Open work remains here. Without another worktree on the target, this simply omits deletion and switching.

If another worktree has the target checked out, its files advance there with its uncommitted work carried over. A conflict in that work refuses the landing. Both worktree chains record their own half; undo in either reverses that half and reports what the other still holds.
