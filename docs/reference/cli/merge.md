# ff merge

Merge another branch into the current one without rewriting either branch's commits. This usually creates a commit with two parents; if the current tip is an ancestor of the target, it fast-forwards instead. `ff merge` refuses the recorded base: use [`ff pull`](pull.md) or [`ff restack`](restack.md) to replay onto it. A branch that already contains a merge of its base can continue that history through [`ff resolve`](resolve.md).

## Usage

```
Usage: ff merge [OPTIONS] <branch>
```

## Examples

```sh
ff merge feature-b              # Take feature-b in by one merge commit
ff merge origin/feature-b       # Take a remote-tracking branch in
ff merge feature-b -m "take b"  # Set the merge's message
ff merge feature-b --resolve    # Open the session if the merge conflicts
ff undo                         # Remove the merge
```

## Options

```
Arguments:
  <branch>
          The branch to take in: a local branch or `origin/<branch>`

Options:
  -m <msg>
          Message for the merge commit; defaults to `merge <branch> into <current>`

      --resolve
          Open the resolution session when the replay on this branch conflicts

      --no-resolve
          Hold on a conflict; overrides fufu.onConflict for this run

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

## What lands

The merge commit has this branch's tip as its first parent and the target's tip as its second. Its tree combines the two parents' changes. The commit has a change ID, your author identity, and a signature when `commit.gpgsign` is enabled. The default subject is `merge <branch> into <current>`; `-m` replaces it. No commit hook runs.

Uncommitted work is reapplied over the result and stays open. If that reapplication conflicts, the merge still lands, but the open work is held as an arrival and the command exits 3. Use `ff resolve` to put that work into the working copy with markers, then edit the files; an arrival has no session to finish with [`ff done`](done.md).

The recorded base stays the same. Dependent branches keep their existing tips; the merge does not replay them.

When this branch's tip is an ancestor of the target, it fast-forwards to the target's tip. No merge commit is written.

## Conflicts

If the merge conflicts, the branch tip stays unchanged and fufu records a held merge. The output names the conflicting commit and files; the exit is 3. Captures and hold metadata may still be written.

Run `ff resolve`, edit the marked files, then run `ff done` to land the merge. `ff done --abandon` closes the session and keeps the hold. `ff resolve --abandon` drops both. Resolution checks the target's current tip, so a moved target can change the conflicts to resolve.

`--resolve` opens the resolution session immediately when the merge conflicts, still with exit 3. Use [`ff config onConflict resolve`](config.md) to make this the default, or `--no-resolve` to stop at the hold for one run.

Opening with `--resolve` takes two operations: one [`ff undo`](undo.md) returns to the branch while leaving the session available; a second removes the session and its newly recorded hold.

## Refusals

The recorded parent, or trunk when no parent is recorded, is refused with `merge/base`. A target already in this branch, including the branch itself, is refused with `merge/nothing`. Histories with no common ancestor are refused with `merge/unrelated`.

An editing session or open resolution session blocks the merge. So does a held absorb, lift, done, or parked-change arrival. A held restack or merge can remain while a clean merge lands, but a second conflicting merge is refused with `held/already-held`. These refusals do not land the requested merge; capture or reconciliation may already have recorded local state.

## After the merge

[`ff push`](push.md) sends the branch as usual. A later restack replays both sides and recomputes the merge, preserving its own edits or conflict resolution where they still apply. Once the merged target is included in the base, the merge can become an ordinary commit or disappear if it adds nothing. See [branches and merges](../../concepts/branches.md).

Pull can replay merges too, but skips its base update for a branch that already contains a merge of that base. Use `ff resolve` on that branch to merge further base updates.

## Undo

One `ff undo` removes a landed merge or fast-forward and restores the working copy.
