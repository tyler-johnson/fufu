# ff resolve

Open the current branch's held rewrite for conflict resolution. A held rewrite is a requested replay waiting on conflicting file changes. Resolve puts the conflicts into the working copy as labeled markers; edit them, then use [`ff done`](done.md) to apply the rewrite and return. On a branch with no hold whose commits hold a merge of its base and that is behind it, resolve takes the base in by a merge.

## Usage

```
Usage: ff resolve [OPTIONS]
```

## Examples

```sh
ff resolve                      # Open the recorded conflicts
# Edit the marked files, then finish the rewrite session:
ff done
ff resolve --abandon            # Alternatively, drop the pending rewrite
```

## Options

```
Options:
      --abandon
          Drop the pending rewrite instead of resolving it

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

## Rewrite sessions

A resolution session is an automatically named branch containing the conflict markers. The hold remains on the original branch, where your previous open work is parked. Switching away parks fixes in progress; switching back resumes them. Done applies the fixes and returns in one operation.

If changed circumstances make the rewrite apply cleanly, resolve releases the hold instead. Re-run the original command to apply that rewrite.

## Taking the base in

A standing hold is opened first. With no hold, resolve reads the branch against its base: when the branch is behind and the commits between its fork and its tip hold a merge of the base, it merges the base in, continuing the shape the branch already chose. A branch on a straight line behind its base is [`ff restack`](restack.md)'s and resolve refuses, naming it; a branch up to date with its base says so; a branch with no base, an editing session, or one sharing no history with its base has nothing to take in.

The merge is one commit with two parents, the branch's tip first and the base's tip second, authored by you with a change ID, signed per `commit.gpgsign`, subject `merge <base> into <branch>`. The branch moves onto it and its open change comes back over it; nothing is rewritten, so [`ff push`](push.md) afterwards is an ordinary push. One [`ff undo`](undo.md) removes it. No hook runs for the merge commit. Only the current branch is merged: from elsewhere, switch to it first.

A conflicting auto-merge records a `merge` hold and opens the resolution session in the same step, exit 3. Edit the markers, then `ff done` lands the merge commit with the fixes as its tree; `ff done --abandon` closes the session and keeps the hold; `ff resolve --abandon` drops both. A held merge follows a later rewrite of the branch the way a held restack does: dropped by a re-aim, cleared by a replay onto its base, and kept otherwise. Running resolve on a held merge that is clean now lands it; on one whose base is already in, it releases the hold. `ff restack --resolve`, [`ff pull --resolve`](pull.md), and [`ff merge --resolve`](merge.md) open the session from the verb the same way.

## Parked-change arrivals

A held arrival occurs when [`ff switch`](switch.md) cannot replay parked work onto a moved branch tip. Resolve handles it in place: the marked files become the open change, with no session and no done step. Fix them and continue ordinary work. Resolution refuses if the branch already has an open change; commit that work first. Switching away only parks it on the same branch, so returning resumes the blocker.

## Abandoning and undo

`--abandon` drops the hold and any open resolution session, returning to the original branch. `ff done --abandon` from the session is the way to close it without dropping the hold. Opening a rewrite-resolution session is two operations, creation and switching; two `ff undo` calls take it back. Landing or abandoning is one operation. [`ff history`](history.md) shows the available steps.

Opening a session over a standing hold exits 0. Exit 3 is owed when this resolve recorded the hold itself: a merge of the base that conflicts. `held/none` exits 3 as every `held/` refusal does.
