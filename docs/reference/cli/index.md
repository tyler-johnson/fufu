# CLI reference

Every command, grouped the way `ff --help` groups them. Each page is the same text `ff help <command>` prints: purpose, usage, examples, options, and details.

<!-- Generated from crates/ff-cli/src/help/ by docsgen.rs; edit the sources, then make docs-gen. -->

## Getting started

- [`ff init`](init.md) — Create a repository or enable fufu in an existing one
- [`ff clone`](clone.md) — Copy a repository and enable fufu snapshots

## Working changes

- [`ff status`](status.md) — Show the working copy status
- [`ff diff`](diff.md) — Show uncommitted file changes as a patch
- [`ff restore`](restore.md) — Discard file edits, or restore files from a revision or snapshot
- [`ff commit`](commit.md) — Record working-copy changes without staging
- [`ff describe`](describe.md) — Set a draft message, reword a commit, or rename a branch

## Inspect history

- [`ff map`](map.md) — Show local branch relationships and parked work (also bare `ff`)
- [`ff log`](log.md) — Show commit history and the open change, with change IDs
- [`ff show`](show.md) — Show a revision and its patch; defaults to the open change
- [`ff evolog`](evolog.md) — Show one change's recorded evolution; defaults to open work
- [`ff history`](history.md) — Show available undo and redo steps for this worktree
- [`ff collide`](collide.md) — Check whether two branches' file changes conflict

## Branches

- [`ff switch`](switch.md) — Switch branches or create new work at trunk
- [`ff branch`](branch.md) — List, create, delete, or prune branches
- [`ff worktree`](worktree.md) — List, create, or remove worktrees and retain their captured history

## Rewrite commits

- [`ff absorb`](absorb.md) — Add uncommitted changes to an existing commit; defaults to HEAD
- [`ff lift`](lift.md) — Reopen a commit's changes as uncommitted work; defaults to HEAD
- [`ff restack`](restack.md) — Replay a branch's commits onto its base; defaults to this branch
- [`ff fold`](fold.md) — Replay this branch into a target and delete it; defaults to trunk
- [`ff edit`](edit.md) — Open a session to edit an existing commit's files
- [`ff done`](done.md) — Apply an editing or resolution session and return to its branch
- [`ff resolve`](resolve.md) — Put a held rewrite's conflicts into the working copy for repair

## Remotes

- [`ff pull`](pull.md) — Update this branch from its base and remote copy
- [`ff push`](push.md) — Send this branch to its remote copy with a lease
- [`ff remote`](remote.md) — List configured remote names and fetch URLs

## Recovery

- [`ff undo`](undo.md) — Restore this worktree's recorded local state one undo step back
- [`ff redo`](redo.md) — Step forward again after an undo
- [`ff op`](op.md) — Inspect recorded operations and recover local state
    - [`ff op log`](op-log.md) — List recorded operations, including captures, newest first
    - [`ff op show`](op-show.md) — Show an operation's ref transitions and file diffstat
    - [`ff op diff`](op-diff.md) — Compare the recorded file trees of two operations
    - [`ff op restore`](op-restore.md) — Restore this worktree's recorded local state at an operation
    - [`ff op revert`](op-revert.md) — Invert an operation's ref transitions if those refs have not moved
    - [`ff op trim`](op-trim.md) — Drop operations past the retention cutoff (fufu.keep, 90d)

## Setup

- [`ff hook`](hook.md) — Install shell and agent integrations on this machine
- [`ff unhook`](unhook.md) — Remove managed shell and agent integrations
- [`ff trigger`](trigger.md) — Snapshot the working copy now
- [`ff watch`](watch.md) — Stream operation-history changes as JSON lines
- [`ff config`](config.md) — List, read, or change fufu settings in Git configuration
- [`ff doctor`](doctor.md) — Check operation history, configuration, and installed integrations
- [`ff git`](git.md) — Run Git with snapshot and policy checks; help is `ff help git`
- [`ff explain`](explain.md) — Look up an error id and see what it means
- [`ff version`](version.md) — Print build identity and cached update availability
- [`ff update`](update.md) — Show update instructions and offer supported installation updates
