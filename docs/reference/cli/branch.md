# ff branch

Lines of work. Bare `ff branch` says what exists, `ff branch <name> [<rev>]` creates one without moving there, and `ff branch -d <name>` takes one away. `ff br` is the short spelling, and `ff bookmark` is jj's name for the same verb.

Naming is not here. [`ff describe -b <name>`](describe.md) names the branch you are on, on the same axis as -m — one verb for saying what work is, whether the subject is the change's description or the branch's name.

## The list

Named branches first, then the anonymous ones — the petnames fufu mints when work starts without a name — kept apart so a petname never reads as something you chose. Each row carries its tip, the subject there, and what is hanging off it: a parked change, a pending description, and how it stands against its upstream.

Then what a remote is holding that is not here: the branches a clone or a fetch left a tracking ref for and no local branch of yours tracks. Those rows wear the brackets too, because the brackets mean a name you can type at [`ff switch`](switch.md), and these are: `ff switch origin/<branch>`, or bare `ff switch <branch>`, is what makes the local branch, tracking the remote's.

The section is bounded the way the map is, with a dim count standing for the rest; --all is that bound spelled off.

## Creating

`ff branch <name>` mints a branch at trunk's tip and leaves you where you stand. `<rev>` forks it elsewhere: a revision puts the tip on that commit, and a branch name puts it on that branch's tip and records the branch as the parent, so the new one has a base to be measured against. `@` puts the tip on the commit under the open change and parks a copy of the open change on the new branch, so `ff switch <name>` resumes it there; nothing moves and nothing parks here.

`ff start` is the verb that also moves there. Creating is one operation, so [`ff undo`](undo.md) takes the whole of it back.

## Deleting

The branch's pointer into the log moves to trash rather than evaporating, its open change — the commit it was holding — stays pinned by that timeline and is named on the way out, and the tip stays pinned by the operation. Nothing local is lost, nothing goes to `refs/stash`, there is no merged-check to argue with, and `ff undo` brings the branch and its timeline back.

The branch's operations themselves stay on the log either way; what goes is the way in through this name.

## A published branch

There is more than the name: a copy on the remote, and a tracking ref and upstream pointing at it. A plain delete leaves all three standing and says so.

`--shared` deletes the copy too, under a lease — only if it still stands where you last saw it — and takes the tracking ref and upstream down with it. That half left the machine: the branch still comes back, and the copy does not.

## Usage

```
Usage: ff branch [OPTIONS] [name] [rev]

Arguments:
  [name]
          Create a branch by this name, and stay where you are

  [rev]
          Where it forks from: a revision, `@`, or a branch; trunk when omitted

Options:
  -d, --delete <branch>
          Delete a branch — its timeline moves to trash, and `ff undo` is enough

      --shared
          Remove the copy on the remote too — that half `ff undo` cannot reach

      --all
          Every remote-only branch, not just the newest few

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --json
          Emit machine-readable JSON

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff branch                        what exists, and what is still anonymous
ff branch --all                  every remote branch too, unbounded
ff branch spike                  a branch at trunk, without moving there
ff branch spike main~2           the same, at a revision
ff branch spike @                at the commit under the open change
ff branch -d old-experiment      remove it (undoable)
ff branch -d spike --shared      the copy on the remote goes too
ff describe -b unicode-cleanup   name the branch you are on
```
