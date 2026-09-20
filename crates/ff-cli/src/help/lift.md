Take changes out of the latest commit and return them to your uncommitted work. By default, the source is `HEAD` and the destination is the open change `@`. Use `--from` to select an earlier commit.

## Examples

```sh
ff lift                         # Reopen the latest commit's changes
ff lift src/parser.rs           # Reopen only this file's changes
ff lift --from HEAD~2           # Take changes from an earlier commit
ff lift --from 'HEAD~2..HEAD'    # Reopen the last two commits
ff lift --from HEAD~3 --into HEAD  # Move content between commits
```

### Options

### Paths, messages, and ranges

Paths select whole files or directory prefixes, without globs or hunk selection. Other content stays in its source. A source emptied by the move is dropped, so lifting all of `HEAD` removes that commit from branch history while keeping its changes on disk.

`--from <revset>` selects a contiguous run on the branch's history. `--into <rev>` selects one target below, above, or inside that run, or the open change. `-m` sets the target's message: a pending description for `@`, a reword for a recorded commit. Otherwise its message stays the same.

`ff lift` and `ff absorb` share the same move engine with different defaults. Lift's default destination is always the open change. Absorb's guard against implicitly targeting trunk applies when committed sources default to a recorded target; it does not apply to lift's default open target. A root commit cannot be emptied and dropped.

Preview a range with `ff log -r 'HEAD~2..HEAD'`. An omitted right endpoint can include other branches. See [Revisions and IDs](../revisions.md#revision-sets-and-grammar).

### Conflicts and dependent branches

Commits between and above the endpoints replay in the same operation, including branches inside that range. Surviving changes keep their change IDs while commit hashes change. A conflicting primary replay records a held rewrite without landing it and exits 3; captures and metadata may still be written. `ff resolve` opens it.

After a successful move, dependent branches replay parent before child. A downstream conflict holds that branch and leaves its dependents alone; the move still lands and currently exits 0. Inspect the cascade report or `ff status`. Branches checked out elsewhere or already held are skipped and named. One `ff undo` takes back the move and its cascade.

### Hooks

A default lift makes content uncommitted, so it runs no `pre-commit`. An explicit move with `@` among its sources does run that hook. `-m` on a recorded target runs message hooks as a reword does. `--no-verify` skips `pre-commit` and `commit-msg`.
