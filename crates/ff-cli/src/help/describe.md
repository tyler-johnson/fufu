Set the draft message for your open change, reword an existing commit, or rename the current branch. With no revision or `-b`, edit the draft message in `$EDITOR`; `-m` sets it directly. `ff desc` is the short spelling.

## Examples

```sh
ff describe -m "parser: handle unicode escapes"  # Draft message
ff describe                     # Edit the draft in $EDITOR
ff describe HEAD~2 -m "parser: fix escapes"  # Existing commit
ff describe -b unicode-cleanup  # Rename the current branch
```

### Options

### Three modes

- Draft message: omit the revision, or use `@`. `ff commit` uses this pending description unless its own `-m` overrides it. Describing updates internal open metadata without advancing branch history, and runs no commit hooks.
- Existing message: name one recorded revision. Its message changes and commits above it re-parent in the same operation, including branches inside that range. Surviving change IDs stay the same; commit hashes change.
- Branch rename: `-b <name>` renames the current branch, whether its old name was automatic or chosen. Its capture history, parked work, and pending description remain associated with it. This mode cannot be combined with a revision or `-m`.

The [revision expression](../revisions.md#revision-sets-and-grammar) must select exactly one member. Without `-m`, message modes open `$EDITOR` with the current text.

### Dependent branches and recovery

After a reword, dependent local branches replay parent before child in the same operation. One `ff undo` takes back the reword and cascade.

A reword preserves its commit's tree, but an already stale dependent branch can conflict. That branch records a hold while the reword stands. This outcome currently exits 0; scripts must inspect `reword.cascade.held`. Branches checked out elsewhere or already held are skipped and named, along with the dependents left alone above them. Use `ff switch` and `ff resolve` on a held branch. A held restack already on the branch stays and follows the reword; a held absorb, lift, or done refuses it.

### Hooks

Rewording a recorded commit runs `prepare-commit-msg` and `commit-msg`; a failing hook refuses it before replay planning. `--no-verify` skips `commit-msg`. No file content is committed, so `pre-commit` does not run. Draft descriptions run their hooks later, when committed.
