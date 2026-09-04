# ff sync

Bring every local branch up to date with the two things it answers to: the base it sits on, and the shared copy of itself on the remote. One fetch, then each branch is replayed onto whatever moved, and the whole run is one operation — one [`ff undo`](undo.md) puts every branch and the working tree back.

Nothing leaves the machine. Sync takes in; [`ff publish`](publish.md) sends, and a push is the one act undo cannot take back. When a branch is ahead of its shared copy, sync says so and leaves it for publish.

## The shared copy

Sync asks two questions of each branch. Have you changed this branch since you last saw its shared copy? If not, the branch follows the shared copy wherever it went, a force-push included.

If you have, is what the shared copy holds beyond you new work, or old versions of yours? New work is taken in and your commits replay on top. Old versions of yours are left alone, and publish replaces them; fufu knows them because it recorded the rewrite, or the publish you undid.

Only a branch tracking the remote this run fetched from gets this half. With `--no-fetch`, or a branch tracking another remote, the branch you are standing on is the only one whose shared copy is read.

## The base

One question: did it move? If so, the branch's commits replay onto where it now stands, and the branches stacked on this one follow, parent before child, the way [`ff restack`](restack.md) does.

Only the branch you are standing on has a working tree, so the others move as refs and objects and touch no file.

## What holds, and what is skipped

A replay that conflicts holds that branch: nothing is written there, the run goes on to the next branch, and [`ff resolve`](resolve.md) on that branch picks it up. The branches above a held one stay put, since their base did not move.

Four kinds of branch are named and left where they stand:

- one checked out in another worktree
- one already holding a rewrite
- one whose commits hold a merge
- one that shares no history with its base

## The report

The branch you are standing on comes first, then one block per other branch that did something: its name on a line of its own, and under it what moved, what held, and what was skipped. A repository with nothing to do reads `nothing to sync`.

With `--json`, the other branches are the `branches` array, one row per branch tagged `Synced`, `Elsewhere`, or `Held`; a `Synced` row carries its `remote` and `base` halves, and `files` and `still_open` on the report describe the run's one working-tree write.

The exit is 3 when any branch held, and the last line names the branch to switch to before resolve.

## Usage

```
Usage: ff sync [OPTIONS]

Options:
      --no-fetch
          Skip the fetch: reconcile with what you already have

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff sync                        fetch, bring every branch up to date
ff sync --no-fetch             the same, with what you already have
ff publish                     send the branch you are on, once it lines up
```
