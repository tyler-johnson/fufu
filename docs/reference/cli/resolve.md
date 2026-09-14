# ff resolve

Open the current branch's held rewrite for conflict resolution. A held rewrite is a requested replay waiting on conflicting file changes. Resolve puts the conflicts into the working copy as labeled markers; edit them, then use [`ff done`](done.md) to apply the rewrite and return.

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

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Rewrite sessions

A resolution session is an automatically named branch containing the conflict markers. The hold remains on the original branch, where your previous open work is parked. Switching away parks fixes in progress; switching back resumes them. Done applies the fixes and returns in one operation.

If changed circumstances make the rewrite apply cleanly, resolve releases the hold instead. Re-run the original command to apply that rewrite.

## Parked-change arrivals

A held arrival occurs when [`ff switch`](switch.md) cannot replay parked work onto a moved branch tip. Resolve handles it in place: the marked files become the open change, with no session and no done step. Fix them and continue ordinary work. Resolution refuses if the branch already has an open change; commit that work first. Switching away only parks it on the same branch, so returning resumes the blocker.

## Abandoning and undo

`--abandon` drops the hold and any open resolution session, returning to the original branch. Opening a rewrite-resolution session is two operations, creation and switching; two [`ff undo`](undo.md) calls take it back. Landing or abandoning is one operation. [`ff history`](history.md) shows the available steps.
