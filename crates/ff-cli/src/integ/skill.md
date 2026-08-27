---
name: fufu
description: Advanced use of fufu (ff), the git interface that snapshots the working tree before every action. Use when recovering file state or a whole tree after a bad edit, undoing or reverting an operation, splitting or reordering commits that have already closed, resolving a held rewrite, re-aiming a branch onto a new base, reading fufu's JSON from a script, or whenever git's usual advice — staging, stash, reflog, rebase -i — would fight fufu's model.
---

# fufu

`ff` is a primary interface to an ordinary git repository. It owns capture, movement, history rewriting, and undo; git owns the durable graph. At every instant the repository is a boring git repository — HEAD attached, ordinary commits, `git status` legible — so reading with git is always fine, and everything below is about writing.

The once-per-session briefing already gave the agent four verbs and the git rule. This is the rest.

## The model

**The worktree is the change.** There is no staging area and no verb that adds to one. What is on disk is what `ff commit` closes. Selection happens as an argument at the moment of the close, not as state maintained between commits.

**Capture is ambient.** Every verb takes a snapshot before it does anything, and the hooks take one before every agent tool call and every typed git command. Snapshots are ordinary git objects under `refs/fufu/`, beside history rather than in it. Nothing fufu stores reaches a remote, and nothing it stores needs fufu to read back. The practical consequence: file state from the last hour is nearly always recoverable, so work directly and never write backup copies.

**Two address spaces, and they never collide.** Commits are hex. Operations are spelled in the letters k–z, never hex digits. `@` is the newest operation, and git's first-parent suffixes work on it: `@^` is the one before, `@~3` is three back. `ff show` takes revisions and refuses operations; `ff op show` takes operations. That separation is why an id is never ambiguous about what kind of thing it names.

**Undo is repo-wide; restore is per-path.** `ff undo` moves refs, HEAD, the index, and the working tree together, one *run* of work at a time. `ff restore <path>` writes only worktree files and leaves refs, HEAD, and the index exactly as they are. Reaching for the wrong one is the most common mistake — see the recovery table below.

**Undo navigates rather than appends.** `ff undo` and `ff op restore <id>` move the log's pointer; nothing is discarded and no entry records that you navigated. `ff redo` walks forward along the branch an undo stepped off. Landing new work after an undo forks the log instead of truncating it, so redo stops offering a path it can no longer take, while the forked-off ids stay resolvable until `ff trim` ages them out.

## Reading

Reading with git is fine and needs no `ff`. These say more than their git counterparts:

- `ff status` — branch, upstream, the open change, and a diffstat. Also where foreign drift is loud: work done behind fufu's back is reported until the next fufu operation absorbs it.
- `ff diff` — the open change as a patch. It is the only patch tool that sees untracked files, which is exactly where a wrong commit comes from.
- `ff log` — commits wearing the id of the operation that built them. `-r` takes a revset; the positional is only ever paths, so `ff log main` asks about a *path* called main. `--commits` drops to plain history.
- `ff show` — one revision with its patch; bare, the open change.
- `ff` — the map: recent work across every branch, parked changes included.
- `ff history` — where you can go back to, one row per `ff undo` press.
- `ff evolog` — every operation on the open change, newest first. This is where a lost hour is found: each row is a whole worktree.
- `ff collide <branch>` — would two branches conflict if both landed? Answered in memory, writes nothing, exit 0 either way.

Every verb takes `-C <dir>`, a chdir, so `ff -C ../bay status` asks another worktree a question without leaving this one.

## Committing

Mechanics only. Message style belongs to the project, not to fufu — follow whatever convention the repository already uses.

- `ff commit -m "…"` closes the open change. No add, no staging.
- `ff commit <path> -m "…"` closes a slice — a file or a directory prefix, no globs — and leaves the rest open, still the change you are in the middle of. That is how one worktree becomes several commits: repeat it, narrowing each time.
- `ff describe -m "…"` sets a pending description on the open change before it is ever a commit; `ff commit` with no `-m` picks it up.
- `ff describe <rev> -m "…"` rewords a commit that has already closed, restacking everything above it.
- `ff commit -b <branch>` lands the close on a branch, claiming an anonymous one or forking a fresh one.
- `ff commit --no-verify` skips pre-commit and commit-msg hooks.

A clean tree has nothing to close. Every close is recorded, so `ff undo` takes it back — tree and refs together.

Moving content between the open change and a commit that has already closed:

- `ff absorb --into <rev>` folds the open change into that commit. Everything above re-parents in the same operation.
- `ff lift --from <rev>` is the other direction: takes files back out of a closed commit and into the open change. If the lift empties the commit, the commit is dropped.

Neither attributes hunks. Whole files are the unit, and a path argument only chooses which files.

## Recovery

Find the id first — `ff history` for undo steps, `ff evolog` for the open change's own operations, `ff op log` for everything. Then:

| Situation | Verb |
| --- | --- |
| Take back the last thing that happened, whole repo | `ff undo`, repeated |
| Go forward again | `ff redo` |
| Land on one named operation, whole repo | `ff op restore <id>` |
| Take back one old change, keeping later work | `ff op revert <id>` |
| Discard edits to a file | `ff restore <path>` |
| A file as it was at an operation | `ff restore <path> --at-op <id>` |
| A file or tree as it was at a time | `ff restore --all --at 2h` |
| A path from another revision | `ff restore <path> --from <rev>` |
| What did that operation change? | `ff op show -p <id>` |
| What changed between two operations? | `ff op diff <a> <b>` |

`--at` takes `30m`, `2h`, `3d`, or a date. A restore captures first, mandatorily, so any restore is undone by another restore or by `ff undo`.

`ff op revert <id>` is the one verb in the `op` family that writes an operation, because inverting a change while later work stands is itself something that happened.

## Rewriting

Descendants rebase automatically. Every verb here is one operation, so one `ff undo` takes the whole thing back.

- `ff edit <rev>` opens an editing session on a commit: a branch is minted there and you switch to it, so the commit's real content is what your toolchain sees. The branch you came from stays where it stands, its commits waiting ahead. Your open change parks and returns when the session ends.
- `ff done` amends the commit with what the worktree now holds, replays what waited onto it, and lands you back. `ff done --abandon` drops the session instead, stashing rather than discarding what is uncommitted.
- `ff restack` replays a branch's commits onto the base it sits on. It takes a branch name, so a branch you are not standing on restacks without touching a file on disk.
- `ff restack --onto <branch>` records a new parent first. This is the only way to re-aim a branch. A base is a branch wherever it lives, so `origin/main` names one too.

A replay that would conflict stops with nothing changed rather than leaving a half-finished rebase on disk.

## Held rewrites and conflicts

A **held rewrite** is a conflict fufu chose not to interrupt you with. The verb that hit it recorded a hold in the branch's metadata and moved nothing. `ff status` reports it, and `ff publish` refuses to send while one stands.

- `ff resolve` materializes every surviving conflict region into the working tree at once, as ordinary labeled markers. Nothing moves — the branch stays, a parked change keeps waiting. Fix the markers, then `ff done` lands the rewrite behind them.
- If the world has moved and the rewrite now applies cleanly, `ff resolve` releases the hold instead, and re-running the verb that recorded it lands it.
- `ff resolve --abandon` drops the hold, and an open session's markers with it.
- Either way, the way back is one `ff undo`.

## Branches, parking, worktrees

- `ff start` begins new work on a fresh branch, forked from trunk; a revision or branch argument forks there instead. The open change parks where it was and the new branch opens clean — nothing is carried across a fork. `ff start` never creates a commit.
- `ff switch <branch>` parks whatever is open with the branch being left and brings back whatever was parked at the destination — same files, same edits, same pending description. There is no stash dance. A unique prefix of the name is enough.
- `ff describe -b <name>` names the branch you are on. Naming is not on `ff branch`, because a plain `git branch -m` would orphan the capture chain, the parked change, and the pending description.
- `ff branch` lists; `ff branch delete <name>` removes one, undoably.
- `ff worktree add <name>` makes a second checkout on a branch of its own. Each worktree has its own operation chain, its own undo, and its own lock.

## Remotes

- `ff sync` fetches, takes in whatever arrived, and replays onto the base. Nothing leaves the machine, and everything it does is undoable. A replay that conflicts holds.
- `ff publish` sends the branch under a lease: the push goes through only if the shared copy still stands where you last saw it. It does not fetch first, on purpose. It is the one thing fufu does that no operation log can take back, which is why it is a verb you type rather than a step riding inside another.
- `ff publish -n` says which of the four pushes this would be — create, replace, restore a deleted copy, or roll one back — while the answer still costs nothing.
- `ff publish --to <remote>` records which remote a branch answers to, once.
- The way back from a bad publish is another publish, not `ff undo`: undo the commit locally, publish again, and the lease rolls the shared copy back.

## Landmines

- **Do not stage.** `ff git add`, `ff git reset`, and `ff git stash` fight the model rather than extending it. Path-limited `ff commit` replaces `add -p`; `ff switch <branch>` replaces stash; `ff restore <path>` replaces `checkout --`.
- **Do not reach for `git rebase -i`.** `ff edit <rev>` with `ff done`, `ff absorb`, `ff lift`, and `ff describe <rev>` cover splitting, squashing, reordering, and rewording, and each is one undoable operation.
- **`ff git <args…>` is the escape hatch, not the default.** It snapshots and then runs git verbatim, so it is always safe, but a fufu verb that covers the same ground keeps the operation on the log where undo can reach it. Use it for what fufu genuinely does not do — `bisect`, `grep`, `tag`, `show HEAD:file`.
- **Only fufu writes fufu state.** Never hand-edit `refs/fufu/*`. Extensions read fufu state and call fufu verbs.
- **A conflict is never left half-applied.** If a verb reports a hold, the repository did not move; do not go looking for a rebase to continue.

## Machine surface

Every verb takes `--json` and emits a versioned envelope, `{"ff": 1, "cmd": "status", …}`, so a script can assert what it is talking to. The JSON is not a mirror of the human layout — `ff status` crops to two rows for an eye while its JSON carries the model whole.

- `ff watch` streams the operation log, one JSON object per line, as it moves. `--all` widens it to every worktree. It is a foreground process, not a daemon.
- `ff explain <id>` looks up an error id; `ff explain --list` shows them all.
- `ff config` lists every setting with its value and default, validated through the readers' own parsers. Storage is plain git config under `fufu.*`.
- `ff doctor` reads the whole safety net in one pass and exits 1 on findings, so CI can gate on it. It is read-only except `--fix`.
- A verb fufu does not know runs `ff-<name>` from PATH, git-style. The child inherits `FF_REPO`, `FF_CONTRACT`, and `FF_SESSION`.

## The authority

Every verb's own `--help` is the last word on its flags and its behavior, and it is long-form and worth reading. This page is routing and the model; the binary is the specification.
