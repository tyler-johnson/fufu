# CLI reference

Every command, grouped the way `ff --help` groups them. Each page is the same text `ff help <command>` prints, with clap's usage block between the two halves. This directory is generated from `crates/ff-cli/src/help/` by a test — edit there, then `make docs-gen`.

## start a working area

- [`ff init`](init.md) — Start a repository with the safety net already on
- [`ff clone`](clone.md) — Clone a repository, and arm it on arrival
- [`ff worktree`](worktree.md) — Worktrees of this repository, and the chains of ones that are gone
    - [`ff worktree list`](worktree-list.md) — Every worktree here, and every chain whose worktree is gone
    - [`ff worktree add`](worktree-add.md) — Make a worktree: a second checkout of this repository, with its own log
    - [`ff worktree remove`](worktree-remove.md) — Take a worktree away, capturing what it holds first

## work on the current change

- [`ff status`](status.md) — Show the working tree status
- [`ff diff`](diff.md) — Show the open change as a patch — content, not just counts
- [`ff restore`](restore.md) — Restore worktree files from the timeline
- [`ff commit`](commit.md) — Close the open change into a commit (the working tree is the change)
- [`ff describe`](describe.md) — Edit the pending description of the open change

## examine the history and state

- [`ff map`](map.md) — The map bare `ff` draws: the local branches as a skeleton
- [`ff log`](log.md) — Show the timeline: commits wearing the operations that built them
- [`ff show`](show.md) — Show one commit: what it was, and what it did
- [`ff evolog`](evolog.md) — Show the open change's operations, newest first (the evolution log)
- [`ff history`](history.md) — Where you can go back to: one row per `ff undo` step, with redo above
- [`ff collide`](collide.md) — Would two branches hit each other if both landed

## grow, mark and tweak your common history

- [`ff start`](start.md) — Begin new work on a fresh branch
- [`ff switch`](switch.md) — Switch branches; a dirty tree is parked, a parked change resumes
- [`ff branch`](branch.md) — Manage lines of work: what exists, and removing one
    - [`ff branch list`](branch-list.md) — Named branches and anonymous ones, kept apart
    - [`ff branch delete`](branch-delete.md) — Delete a branch — its timeline moves to trash, and `ff undo` is enough
- [`ff absorb`](absorb.md) — Fold working changes into a commit that has already closed
- [`ff lift`](lift.md) — Take changes back out of a closed commit, into the open change
- [`ff restack`](restack.md) — Replay a branch's commits onto the base it sits on
- [`ff edit`](edit.md) — Open an editing session on a commit: go there, edit it, come back
- [`ff done`](done.md) — Finish the editing session: amend, replay what waited, land back
- [`ff resolve`](resolve.md) — Materialize a held rewrite's conflicts and fix them, all at once

## collaborate

- [`ff sync`](sync.md) — Line this branch up with its base and its remote
- [`ff publish`](publish.md) — Send this branch to its remote, under a lease
- [`ff remote`](remote.md) — What the remotes here are called, and where each one points

## go back

- [`ff undo`](undo.md) — Step the whole repository back one run of work
- [`ff redo`](redo.md) — Step forward again after an undo
- [`ff op`](op.md) — The operation log as objects: read it, compare it, move to it
    - [`ff op log`](op-log.md) — Every operation, newest first, with the ids these verbs take
    - [`ff op show`](op-show.md) — Show one operation: what it was, what it moved, what it holds
    - [`ff op diff`](op-diff.md) — Compare the worktrees two operations carry
    - [`ff op restore`](op-restore.md) — Rewind the whole repository to an operation
    - [`ff op revert`](op-revert.md) — Invert one operation, leaving later work standing
- [`ff trim`](trim.md) — Drop operations past the retention cutoff (fufu.keep, 90d)

## wire it in, and check on it

- [`ff hook`](hook.md) — Hook fufu into the agent clients and shells on this machine
- [`ff unhook`](unhook.md) — Remove exactly what hook added
- [`ff trigger`](trigger.md) — Snapshot the working tree now
- [`ff watch`](watch.md) — Stream operations as they land, one JSON object per line
- [`ff mcp`](mcp.md) — Serve fufu to an agent client over the Model Context Protocol, on stdio
- [`ff config`](config.md) — Read and write fufu's settings (plain git config under fufu.*)
- [`ff doctor`](doctor.md) — Verify the safety net: the log, identity, reflogs, gc guard, wiring

## fufu itself

- [`ff git`](git.md) — Capture-first git passthrough; fufu.gitPolicy decides what it says
- [`ff explain`](explain.md) — Look up an error id and see what it means
- [`ff version`](version.md) — Which fufu this is, and whether it is the current one
- [`ff update`](update.md) — Name the command that updates this fufu, and offer to run it
