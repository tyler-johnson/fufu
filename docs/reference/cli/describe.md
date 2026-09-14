# ff describe

Set the draft message for your open change, reword an existing commit, or rename the current branch. With no revision or `-b`, edit the draft message in `$EDITOR`; `-m` sets it directly. `ff desc` is the short spelling.

## Usage

```
Usage: ff describe [OPTIONS] [rev]
```

## Examples

```sh
ff describe -m "parser: handle unicode escapes"  # Draft message
ff describe                     # Edit the draft in $EDITOR
ff describe HEAD~2 -m "parser: fix escapes"  # Existing commit
ff describe -b unicode-cleanup  # Rename the current branch
```

## Options

```
Arguments:
  [rev]
          The revision to reword; omitted describes the open change

Options:
  -m <msg>
          The description text; omitted opens $EDITOR

  -b <branch>
          Rename the current branch instead of editing a message

      --no-verify
          Skip commit-msg when rewording a recorded commit

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

## Three modes

- Draft message: omit the revision, or use `@`. [`ff commit`](commit.md) uses this pending description unless its own `-m` overrides it. Describing updates internal open metadata without advancing branch history, and runs no commit hooks.
- Existing message: name one recorded revision. Its message changes and commits above it re-parent in the same operation, including branches inside that range. Surviving change IDs stay the same; commit hashes change.
- Branch rename: `-b <name>` renames the current branch, whether its old name was automatic or chosen. Its capture history, parked work, and pending description remain associated with it. This mode cannot be combined with a revision or `-m`.

The [revision expression](../revisions.md#revision-sets-and-grammar) must select exactly one member. Without `-m`, message modes open `$EDITOR` with the current text.

## Dependent branches and recovery

After a reword, dependent local branches replay parent before child in the same operation. One [`ff undo`](undo.md) takes back the reword and cascade.

A reword preserves its commit's tree, but an already stale dependent branch can conflict. That branch records a hold while the reword stands. This outcome currently exits 0; scripts must inspect `reword.cascade.held`. Branches checked out elsewhere, already held, or containing merges are skipped and named, along with the dependents left alone above them. Use [`ff switch`](switch.md) and [`ff resolve`](resolve.md) on a held branch.

## Hooks

Rewording a recorded commit runs `prepare-commit-msg` and `commit-msg`; a failing hook refuses it before replay planning. `--no-verify` skips `commit-msg`. No file content is committed, so `pre-commit` does not run. Draft descriptions run their hooks later, when committed.
