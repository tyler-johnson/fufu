# ff merge

Merge a branch into the current one by one commit with two parents, the current tip first, and take its tree in without rewriting either side. Use it for a branch you do not own whose work you need: a replay onto it would put your commits on its older trunk. The base is never merged. [`ff pull`](pull.md) brings the base in and [`ff restack`](restack.md) replays onto it; a branch whose history already holds a merge of its base continues that shape through [`ff resolve`](resolve.md).

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

One commit with two parents, this branch's tip first and the target's second, whose tree is the auto-merge of the two. It is fufu's commit: a change id, your signature, and `commit.gpgsign` honored. The default subject is `merge <branch> into <current>`; `-m` replaces it. The open change rides the merge the way it rides a restack and stays open. The recorded base does not change, so a branch on `main` still sits on `main`. Branches stacked on this one are unaffected: the merge lands above their fork. No hook runs.

When this branch has no commits of its own above the fork with the target, the branch fast-forwards to the target's tip and the output says so. No merge commit is written.

## Conflicts

A conflicting auto-merge writes nothing. The merge is recorded as a held rewrite, the output names the commit and the files, and the exit is 3. `ff resolve` opens a session with the conflicts as markers, [`ff done`](done.md) lands the merge with the fixes, and `ff done --abandon` closes the session and keeps the hold; `ff resolve --abandon` drops both. The target is resolved fresh when the hold lands, so a target that moved is what lands.

`--resolve` opens the session in the same run: the hold is recorded, HEAD moves onto the session, and the exit is still 3. `fufu.onConflict resolve` makes `--resolve` the standing choice and `--no-resolve` holds for one run. One [`ff undo`](undo.md) returns to the branch with the session open; a second removes the session and the hold together.

## Refusals

The base, the recorded parent or trunk, refuses with `merge/base`. A target already in this branch, or this branch itself, refuses with `merge/nothing`. Two histories with no common ancestor refuse with `merge/unrelated`. An editing session refuses; a held absorb, lift, or done refuses; a held restack or merge stands, and a second conflicting merge under it refuses with `held/already-held`. A refusal writes nothing.

## After the merge

[`ff push`](push.md) sends the branch as usual. A later `ff restack` or `ff pull` carries the merge: both parents are mapped through the replay and the merge is re-merged, and once the target lands in the base its parent falls beneath the range and the merge collapses into a straight line. Pull's base axis skips a branch that holds a merge of its base, not one that holds a merge of another tree.

## Undo

One `ff undo` removes the merge, or the fast-forward, and puts the working copy back.
