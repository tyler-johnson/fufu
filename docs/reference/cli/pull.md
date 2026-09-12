# ff pull

Line a branch up with the two things it answers to: the base it sits on, and the shared copy of itself on the remote. Both halves, every run — what arrived on the remote is taken in, and a branch whose base moved beneath it is replayed onto where that base now stands. One fetch opens the run, then each branch in it is replayed onto whatever moved, and the whole run is one operation — one [`ff undo`](undo.md) puts every branch and the working copy back. `ff sync` is an alias: fufu's older word for this verb, kept for the fingers that learned it.

Nothing leaves the machine. This takes in; [`ff push`](push.md) sends, and a push is the one act undo cannot take back. When a branch is ahead of its shared copy, this says so and leaves it for the outgoing half.

## Which branches

Bare, this is the branch you stand on. Names take one or more branches instead, and `--all` is every local branch. A branch in the run brings the local bases beneath it in with it, down to trunk, each brought level with its own shared copy first: that is how a teammate's commit on `main` reaches the branch you stand on, and local `main` moves with it whether trunk is spelled `main` or `origin/main`. A name resolves the way [`ff restack`](restack.md) resolves one, an unambiguous prefix included, and a name no branch answers to is refused before the fetch.

What is stacked above a branch in the run is not in the run: its shared copy is not read, and it moves only when a replay beneath it carries it, the way it would under `ff restack`. Name it, or run `--all`, to pull it on its own account.

## The shared copy

Two questions of each branch. Have you changed this branch since you last saw its shared copy? If not, the branch follows the shared copy wherever it went, a force-push included.

If you have, is what the shared copy holds beyond you new work, or old versions of yours? New work is taken in and your commits replay on top. Old versions of yours are left alone, and `ff push` replaces them; fufu knows them because it recorded the rewrite, or the push you undid.

Only a branch tracking the remote this run fetched from gets this half. With `--no-fetch` — the global flag, the same one that keeps any verb from fetching — or a branch tracking another remote, the branch you are standing on is the only one whose shared copy is read. A branch you are not standing on that only fast-forwards moves as a ref, and nothing above it follows; a replay carries what is stacked above it, as every replay does.

## The base

One question: did it move? If so, the branch's commits replay onto where it now stands, and the branches stacked on this one follow, parent before child, the way `ff restack` does. This half runs whether or not there is a remote at all.

Only the branch you are standing on has a working copy, so the others move as refs and objects and touch no file.

## What holds, and what is skipped

A replay that conflicts holds that branch: nothing is written there, the run goes on to the next branch, and [`ff resolve`](resolve.md) on that branch picks it up. The branches above a held one stay put, since their base did not move.

Four kinds of branch are named and left where they stand:

- one checked out in another worktree
- one already holding a rewrite
- one whose commits hold a merge
- one that shares no history with its base

## Dry run

`--dry-run` (`-n`) says what the run would do and writes none of it. Every branch in the run is planned the way a real run plans it, so the report says which would fast-forward, which would replay onto a moved base or a moved shared copy and how many commits, which would hold on a conflict and where, and which would be skipped and why, in the same shape with every sentence in the conditional. No branch moves, no file is written, no hold is recorded, and nothing goes on the operation log, so there is nothing to undo and the closing line says so.

The fetch still runs. Without it the report could not say what the shared copy holds, and a fetch writes the remote-tracking refs under `refs/remotes/<remote>/` and the objects behind them, what [`ff git fetch`](git.md) writes, prunes the tracking refs of copies the remote no longer has, and touches no local branch, no file, and no operation. `--dry-run --no-fetch` reads what you already have and writes nothing at all. The exit is 3 when a branch would hold, the same as a real run, so a script can ask before it pulls.

## The report

The branch you are standing on comes first, then one block per other branch in the run that did something: its name on a line of its own, and under it what moved, what held, and what was skipped. A run with nothing to do reads `nothing to pull`. When names left the branch you stand on out of the run, it says nothing, and neither does the line about what it has waiting to push.

With `--json`, the other branches in the run are the `branches` array, one row per branch tagged `Pulled`, `Elsewhere`, or `Held`; a `Pulled` row carries its `remote` and `base` halves, and `files` and `still_open` on the report describe the run's one working-copy write. The branch you stand on has `remote` and `base` of its own, and both read `NotNamed` when the run did not reach it. `dry_run` on the report says whether the run wrote anything, and under a dry run the envelope's `undo` is null and `files` is the count the write would have touched.

The exit is 3 when any branch held, and the last line names the branch to switch to before resolve.

## Usage

```
Usage: ff pull [OPTIONS] [branch]...

Arguments:
  [branch]...
          Branches to pull, each with the bases beneath it; without any, the one you are on

Options:
      --all
          Every local branch

  -n, --dry-run
          Say what would move, hold, and be skipped, without writing it

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff pull                        fetch, then line this branch up
ff pull side                   the same for side, from wherever you stand
ff pull a b                    two branches, one fetch, one operation
ff pull --all                  every local branch
ff pull -n                     say what would move, hold, and be skipped
ff pull --no-fetch             with what you already have
ff push                        send the branch you are on, once it lines up
```
