# ff done

Finish the current editing or rewrite-resolution session and return to the original branch. For [`ff edit`](edit.md), amend the selected commit and replay later commits. For [`ff resolve`](resolve.md), apply the conflict fixes to the rewrite they belong to.

## Usage

```
Usage: ff done [OPTIONS]
```

## Examples

```sh
ff done                         # Apply the session and return
ff done --abandon               # Return without applying it
ff done --no-verify             # Skip pre-commit and commit-msg
```

## Options

```
Options:
      --abandon
          Return without applying the session; retain its captured work

      --no-verify
          Skip pre-commit and commit-msg hooks

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

## Abandoning and conflicts

`--abandon` drops the session without applying it or running commit hooks. Uncommitted session work remains as an internal open commit pinned by the operation, named in the report and recoverable with [`ff undo`](undo.md). On a resolution session `--abandon` closes the session and keeps the hold: the branch stays held, `ff resolve` opens a fresh session over it, and `ff resolve --abandon` drops both.

A conflicting primary replay leaves the session open without landing it and exits 3. Captures and metadata may still be written. A held restack on the landing branch stays and follows the rewrite; a held absorb, lift, or done there refuses the landing. A held parked-change arrival resolves in place and has no session to finish with done.

## Dependent branches and undo

After a successful landing, dependent branches replay parent before child in the same operation. A downstream conflict holds that branch, leaving dependents above it alone; the session still lands and currently exits 0. Inspect the cascade report or [`ff status`](status.md). Branches checked out elsewhere or already held are skipped and named.

One undo restores the session before its landing or abandonment, including the cascade. Opening it is separate: a rewrite-resolution opening takes two operations, so undo once returns from the session and again removes it.

## Hooks

Landing runs `pre-commit` over the content being recorded, including resolution sessions. A new description also runs message hooks. A failing hook leaves the session open. `--no-verify` skips `pre-commit` and `commit-msg`.
