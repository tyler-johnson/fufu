# ff branch delete

The branch's pointer into the log moves to trash rather than evaporating, its parked change is demoted to an ordinary stash entry, and the tip stays pinned by the operation — so nothing local is lost and there is no merged-check to argue with. [`ff undo`](undo.md) brings the branch and its timeline back.

A published branch is more than the name, though: there is a copy on the remote, and a tracking ref and upstream pointing at it. A plain delete leaves all three standing and says so — undo has to be exact, and it cannot reach any of them. `--shared` deletes the copy too, under a lease, and takes the tracking ref and upstream down with it. That half left the machine; the branch still comes back, and the copy does not.

The branch's operations themselves stay on the log either way; what goes is the way in through this name.

## Usage

```
Usage: ff branch delete [OPTIONS] <branch>

Arguments:
  <branch>
          The branch to delete, by its full name

Options:
      --shared
          Remove the copy on the remote too — that half `ff undo` cannot reach

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
ff branch delete old-experiment
ff branch delete ff/misty-owl    an anonymous one you are done with
ff branch delete spike --shared  the copy on the remote goes too
ff undo                          put the branch back
```
