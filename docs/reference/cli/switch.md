# ff switch

Switch to a branch, or create a branch for new work. With no target, create an automatically named branch at trunk's tip. Uncommitted work stays parked with the branch you leave; work parked on the destination resumes there. `ff sw`, `ff start`, and `ff new` are aliases.

## Usage

```
Usage: ff switch [OPTIONS] [target]
```

## Examples

```sh
ff switch main                  # Continue a local branch
ff switch origin/spike          # Create a local tracking branch
ff start                        # New branch at trunk
ff start -b hotfix              # New branch named hotfix
ff switch main -b               # Fork main with an automatic name
ff start HEAD~2 -b experiment    # New branch at a revision
ff start @ -b spike             # Copy the open change to a new branch
ff undo                         # Reverse the switch and branch creation
```

## Options

```
Arguments:
  [target]
          Local branch, remote branch, or revision; omit to create at trunk

Options:
  -m <msg>
          Pending description for the change being opened

  -b [<name>]
          Create a branch with an optional name; put bare -b after the target

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

## Target behavior

```text
Target                  Result
none                    New automatically named branch at trunk
local branch            Continue it; a unique prefix is accepted
remote branch           Create a local branch tracking that copy
revision                New automatically named branch at that commit
branch with -b          Fork it; record that branch as the base
@                       Fork below the open change and copy its edits
```

Lookup tries a local branch, then a remote branch, then a revision. Remote branches accept a qualified name such as `origin/spike`, or an unambiguous bare name. A new tracking branch sends future [`ff push`](push.md) updates to that remote copy.

`-b` accepts an optional name. Put a bare `-b` after the target: `ff switch main -b` forks main, while `ff switch -b main` requests a new branch named main at trunk. `-m` supplies the new open change's description and is refused when the switch opens no new change.

## Parked work and conflicts

A parked change contains files, edits, and its pending description. The index is not carried: staged hunks return as unstaged edits. A new branch starts clean except when the target is `@`: that copies the open change while leaving the original parked on the branch you left.

When a destination tip has moved, its parked change replays onto that tip. A conflict still completes the switch and records a held arrival, exiting 3. [`ff status`](status.md) reports it. [`ff resolve`](resolve.md) puts markers in the working copy in place; edit them there, without a [`ff done`](done.md) session. `ff resolve --abandon` drops the held arrival.

## Storage and recovery

Switching does not record a commit in branch history. Parked work uses an internal open commit under `refs/fufu/open/<branch>`, visible to `git log --all`. The switch, any new branch, and any copied open change are one operation, reversible together with [`ff undo`](undo.md).
