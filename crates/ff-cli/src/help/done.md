Finish the current editing or rewrite-resolution session and return to the original branch. For `ff edit`, amend the selected commit and replay later commits. For `ff resolve`, apply the conflict fixes to the rewrite they belong to.

## Examples

```sh
ff done                         # Apply the session and return
ff done --abandon               # Return without applying it
ff done --no-verify             # Skip pre-commit and commit-msg
```

### Options

### Abandoning and conflicts

`--abandon` drops the session without applying it or running commit hooks. Uncommitted session work remains as an internal open commit pinned by the operation, named in the report and recoverable with `ff undo`.

A conflicting primary replay leaves the session open without landing it and exits 3. Captures and metadata may still be written. A held parked-change arrival resolves in place and has no session to finish with done.

### Dependent branches and undo

After a successful landing, dependent branches replay parent before child in the same operation. A downstream conflict holds that branch, leaving dependents above it alone; the session still lands and currently exits 0. Inspect the cascade report or `ff status`. Branches checked out elsewhere, already held, or containing merges are skipped and named.

One undo restores the session before its landing or abandonment, including the cascade. Opening it is separate: a rewrite-resolution opening takes two operations, so undo once returns from the session and again removes it.

### Hooks

Landing runs `pre-commit` over the content being recorded, including resolution sessions. A new description also runs message hooks. A failing hook leaves the session open. `--no-verify` skips `pre-commit` and `commit-msg`.
