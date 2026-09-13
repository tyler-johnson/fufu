---
name: fufu
description: Advanced use of fufu (ff), the git interface with automatic working-copy snapshots. Use when recovering file state or a whole tree after a bad edit, undoing or reverting an operation, splitting or reordering commits that have already closed, resolving a held rewrite, re-aiming a branch onto a new base, reading fufu's JSON from a script, or whenever git's usual advice — staging, stash, reflog, rebase -i — would fight fufu's model.
---

# fufu

`ff` provides capture, branch movement, history rewriting, and undo over an ordinary Git repository. Reading with Git is fine.

The once-per-session briefing already gave the agent four verbs and the git rule. This is the rest.

## The model

**The working copy is the change.** There is no staging area and no verb that adds to one. What is on disk is what `ff commit` closes. Selection happens as an argument at the moment of the close, not as state maintained between commits.

**Capture is normally automatic.** Repository readers attempt a snapshot; mutating verbs capture after initial guards and before local changes. Active agent hooks capture the events they receive; shell integration adds prompt captures and a git alias. Help, watch, setup commands, and some dry runs skip capture. `ff trigger -m "checkpoint"` takes a manual snapshot. Recovery requires a successful, retained capture: ignored untracked files, unsaved buffers, and regular files above `fufu.maxFileSize` (50 MiB by default, including modified tracked files) may be absent. `fufu.keep` defaults to 90 days. Snapshots are ordinary git objects under `refs/fufu/`, outside branch history and not sent by ordinary pushes.

**Two address spaces.** Commit hashes and operation IDs are hex; change IDs use k–z and survive rewrites. The argument position decides which kind of ID is accepted. In an operation position, `@` is the newest operation, `@^` the one before, and `@~3` three back. In a revision position, `@` is the open change. `ff show` takes revisions; `ff op show` takes operations.

**Undo follows this worktree's chain; restore is per-path.** `ff undo` moves recorded local refs, HEAD, the index, and files together, one *run* at a time, subject to worktree guards. It cannot undo a remote push or recover uncaptured content. `ff restore <path>` writes only worktree files and leaves refs, HEAD, and the index as they are.

**Undo navigates rather than appends.** `ff undo` and `ff op restore <id>` move the log's pointer; `ff redo` walks forward again. New work after undo forks the log and ends the redo path, but the old IDs remain resolvable until `ff op trim` ages them out.

## Reading

Reading with git is fine and needs no `ff`. These say more than their git counterparts:

- `ff status` — branch, upstream, the open change, and a diffstat. Also where foreign drift is loud: work done behind fufu's back is reported until the next fufu operation absorbs it.
- `ff diff` — the open change as a patch. It is the only patch tool that sees untracked files, which is exactly where a wrong commit comes from.
- `ff log` — commits with their change IDs and commit hashes. `-r` takes a revset; the positional is only ever paths, so `ff log main` asks about a *path* called main. `--commits` drops to plain history.
- `ff show` — one revision with its patch; bare, the open change.
- `ff` — the map: recent work across every branch, parked changes included.
- `ff history` — where you can go back to, one row per `ff undo` press.
- `ff evolog` — every operation on the open change, newest first. This is where a lost hour is found: each row is a whole worktree.
- `ff collide <branch>` — would two branches conflict if both landed? The comparison is in memory and exits 0 either way; the CLI can still capture, fetch, and run maintenance.

Every verb takes `-C <dir>`, a chdir, so `ff -C ../bay status` asks another worktree a question without leaving this one.

## Committing

Mechanics only. Message style belongs to the project, not to fufu — follow whatever convention the repository already uses.

- `ff commit -m "…"` closes the open change. No add, no staging.
- `ff commit <path> -m "…"` closes a slice — a file or a directory prefix, no globs — and leaves the rest open, still the change you are in the middle of. That is how one worktree becomes several commits: repeat it, narrowing each time.
- `ff describe -m "…"` rewrites the internal open commit's pending description without adding a commit to branch history; `ff commit` with no `-m` picks it up.
- `ff describe <rev> -m "…"` rewords a commit that has already closed, restacking everything above it.
- `ff commit -b <branch>` lands the close on a branch, claiming an anonymous one or forking a fresh one.
- `ff commit --no-verify` skips pre-commit and commit-msg hooks.

A clean tree has nothing to close. Every close is recorded, so `ff undo` takes it back — tree and refs together.

Moving content between commits, the open change being one of them. `ff absorb` and `ff lift` are one move: `--from <revset>` names a contiguous run of source commits, `--into <rev>` the target, and each verb's word is its defaults. Everything between and above replays in the same operation, and a source the move empties is dropped and named.

- `ff absorb` moves from the open change into the commit under it; `--into <rev>` aims further back, and `--from HEAD~2..HEAD` folds the last two commits into the one under them.
- `ff lift` moves from the commit under the open change into the open change; `--from <rev>` takes from further back, and `--into <rev>` lands it in a closed commit instead.
- `-m "…"` on either gives the target a message: a reword for a closed commit, the pending description for the open change.

Neither attributes hunks. Whole files are the unit, and a path argument only chooses which files.

## Recovery

Find the id first — `ff history` for undo steps, `ff evolog` for the open change's own operations, `ff op log` for everything. Then:

| Situation | Verb |
| --- | --- |
| Take back the last recorded step in this worktree | `ff undo`, repeated |
| Go forward again | `ff redo` |
| Restore this worktree's recorded state at an operation | `ff op restore <id>` |
| Take back one old change, keeping later work | `ff op revert <id>` |
| Discard edits to a file | `ff restore <path>` |
| A file as it was at an operation | `ff restore <path> --at-op <id>` |
| A file or tree as it was at a time | `ff restore --all --at 2h` |
| A path from another revision | `ff restore <path> --from <rev>` |
| What did that operation change? | `ff op show -p <id>` |
| Which files changed between two operations? | `ff op diff <a> <b>` |

`--at` takes `30m`, `2h`, `3d`, or a date. A restore captures first, mandatorily, so any restore is undone by another restore or by `ff undo`.

`ff op revert <id>` is the one verb in the `op` family that writes an operation, because inverting a change while later work stands is itself something that happened.

## Rewriting

Descendants rebase automatically, on both axes: the commits above the one you touched re-parent, and every local branch whose base is the branch that moved is replayed onto its new tip, parent before child, through the whole tree. It all rides the verb's one operation, so one `ff undo` takes it back.

- `ff edit <rev>` opens an editing session on a commit: a branch is minted there and you switch to it, so the commit's real content is what your toolchain sees. The branch you came from stays where it stands, its commits waiting ahead. Your open change parks and returns when the session ends.
- `ff done` amends the commit with what the worktree now holds, replays what waited onto it, and lands you back. `ff done --abandon` drops the session instead; captured edits remain recoverable from the operation log, not the stash list.
- `ff restack` replays a branch's commits onto its base, and the branches stacked on it follow. Restacking another branch leaves this worktree's files alone unless the cascade reaches the current branch. It can auto-fetch first; `--no-fetch` skips that fetch.
- `ff restack --onto <branch>` records a new parent first. This is the only way to re-aim a branch. A base is a branch wherever it lives, so `origin/main` names one too.

A conflicting primary replay records a hold without landing that rewrite; captures and metadata can still be written. In a cascade, earlier successful branch replays stand. Pull, restack, done, lift, and absorb report held branches with exit 3. Describe currently reports a held cascade with exit 0, so inspect its report too.

## Held rewrites and conflicts

A **held rewrite** records a conflicting replay that has not landed on that branch. `ff status` reports it and `ff push` blocks that branch. Use `ff switch <branch>` to reach a hold on a branch above.

- `ff resolve` materializes every surviving conflict region at once, as ordinary labeled markers, on a session branch it mints and switches you to. The hold stays on the branch you left, and your open change parks there. Fix the markers, then `ff done` lands the rewrite and returns you. A switch away parks the fixes; switching back resumes them.
- If the world has moved and the rewrite now applies cleanly, `ff resolve` releases the hold instead, and re-running the verb that recorded it lands it.
- `ff resolve --abandon` drops the hold, and an open session with it, from either branch.
- Opening a rewrite resolution takes two operations: one undo returns from the session, and a second removes it. Landing or abandoning it is one undoable operation. Resolving a held parked-change arrival edits in place and is one operation.
- A cascade leaves alone, and names, a branch checked out in another worktree, one already holding a rewrite, and one whose commits hold a merge. `ff restack <branch>` replays it once it is free.

## Branches, parking, worktrees

- `ff start` is a spelling of `ff switch`, and the rule under both is: find the branch, else mint it. A local branch is continued; a branch a remote holds (`spike` or `origin/spike`) is minted here under its own name, tracking it; a revision mints an anonymous branch at that commit; no target mints one at trunk's tip. `-b` forks a branch target instead of continuing it, or names the mint. The open change parks where it was and the new branch opens clean, except under `@`, which carries a copy of the open change onto the new branch. Neither spelling creates a commit, and every one is one operation.
- `ff switch <branch>` parks whatever is open with the branch being left and brings back whatever was parked at the destination — same files, same edits, same pending description. A park is the branch's open commit, and nothing goes to `git stash list`. If the destination's tip moved under its park and the replay conflicts, the switch exits 3 with the branch held: `ff resolve` lays the change into the working copy with markers, `ff resolve --abandon` drops it. A unique prefix of the name is enough, and a branch only a remote holds is a target by name.
- `ff describe -b <name>` names the branch you are on. Naming is not on `ff branch`, because a plain `git branch -m` would orphan the capture chain, the open commit, and the pending description.
- `ff branch` lists; `ff branch -d <name>` removes one, undoably.
- `ff worktree <path>` makes a second checkout on a branch of its own. Each worktree has its own operation chain, its own undo, and its own lock.

## Remotes

- `ff pull` lines up the current branch and its bases with their remote copies and bases, parent before child. `ff pull <branch>...` selects branches; `ff pull --all` selects every local branch. Local branch and file changes are one undoable operation; fetched objects, tracking refs, and tags are separate. A branch whose replay conflicts holds and the run continues; exit 3 says one did.
- `ff pull -n` previews local replays without moving local branches or files or recording holds. The fetch still writes objects, tracking refs, and tags; `--no-fetch` uses existing refs. Successful dry runs can still trim and run update maintenance.
- `ff push` sends the current branch; `ff push <branch>...` sends those named. Each wire lease requires the remote ref to match the expected tip. Replacing commits also requires the tracking tip to match fufu's seen record; fast-forwards can proceed without that agreement. Auto-fetch can run first without refreshing the seen record. `--no-fetch` skips it. A multi-branch refusal affects that branch and exits 1; blocked holds exit 3 unless a refusal makes it 1. There is no `--all`, branch-ownership check, or special protection for main. Pushed commits can be rewritten; team policy and server protections govern permission to send them.
- `ff push -n` previews the remote update without sending it. Automatic fetching and maintenance can still run.
- `ff push --to <remote>` records which remote a branch answers to, once.
- The way back from a bad push is another push, not `ff undo`: undo the commit locally, push again, and the lease rolls the shared copy back.

## Landmines

- **Neither staging nor `rebase -i` has a place here.** `ff commit <path>`, `ff switch <branch>`, and `ff restore <path>` cover `add -p`, stash, and `checkout --`; `ff edit <rev>` with `ff done`, `ff absorb`, `ff lift`, and `ff describe <rev>` cover splitting, squashing, reordering, and rewording.
- **Only fufu writes fufu state.** Never hand-edit `refs/fufu/*`. Extensions read fufu state and call fufu verbs.
- **A conflict is never left half-applied.** A hold means nothing was written on the branch it names, so there is no rebase in progress to go looking for.

## Raw git, and what fufu says about it

`fufu.gitPolicy` governs `ff git …` and raw Git calls routed through active agent hooks. Permitted commands run verbatim. Strict passthrough refusals happen before capture; agent hooks attempt capture before policy evaluation.

- **observe** — records it, says nothing.
- **coach** (the default) — names the fufu verb the first time each git word comes up in a session. `git commit` earns `ff commit`, `git stash` earns `ff switch <branch>`, `git push` earns `ff push`.
- **strict** — refuses those words and says what to run instead. `ff git commit` exits 2 rather than running, and an agent's raw `git commit` is denied before it starts.

Only the git words fufu actually has a verb for are ever touched; everything else, and anything fufu cannot read with certainty, runs capture-first under every tier, which is what keeps `ff git <args…>` an honest escape hatch.

`ff doctor` reports what the lane has seen. `ff config gitPolicy <tier>` moves it.

## Machine surface

Every verb takes `--json` and emits a versioned envelope, `{"ff": 1, "cmd": "status", …}`, so a script can assert what it is talking to. The JSON is not a mirror of the human layout — `ff status` crops to two rows for an eye while its JSON carries the model whole.

- `ff watch` streams the operation log, one JSON object per line, as it moves. `--all` widens it to every worktree. It is a foreground process, not a daemon.
- `ff explain <id>` looks up an error id; `ff explain --list` shows them all.
- `ff config` lists every setting with its value and default, validated through the readers' own parsers. Storage is plain git config under `fufu.*`.
- `ff doctor` exits 1 on findings. The CLI attempts capture and reconciliation before checking, fetches when enabled, and can run trimming and update maintenance afterward. `--fix` repairs gc keys, dead branch config, managed hooks, and stale shipped skills.
- A verb fufu does not know runs `ff-<name>` from PATH, git-style. The child inherits `FF_REPO`, `FF_CONTRACT`, and `FF_SESSION`.

## The authority

Every verb's own `--help` is the last word on its flags and its behavior, and it is long-form and worth reading. This page is routing and the model; the binary is the specification.
