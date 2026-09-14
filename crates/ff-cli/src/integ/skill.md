---
name: fufu
description: Advanced fufu (ff) for recovering files or whole state, undoing or reverting operations, splitting or rewriting commits, resolving held rewrites, changing a branch's base, and scripting JSON. Use when Git advice about staging, stash, reflog, or rebase -i would conflict with fufu's model.
---

# fufu

Use `ff` for version-control writes; reading with Git is fine. Start with your task:

- Lost edits or a bad operation: **Recovery** below.
- Split uncommitted work: **Committing**. Move content between recorded commits: **Rewriting**.
- A command reports a hold: **Held rewrites and conflicts**.
- Switch branches or use another checkout: **Branches and worktrees**.
- Automate commands: **JSON output and scripting**. Read each verb's `--help` before choosing flags.

## Working copy and IDs

**The open change is uncommitted work.** `ff commit` records eligible working-copy content in branch history without staging. Paths select a partial commit. Inspect `ff diff` and capture warnings first. Internal open commit objects have not yet entered branch history.

**Recovery requires a successful, retained capture.** Readers attempt snapshots; mutators capture after initial guards. Active hooks cover received events. Help, watch, setup commands, and some dry runs skip capture. `ff trigger -m "checkpoint"` takes a manual snapshot; unchanged content adds none. Ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` (50 MiB by default, including modified tracked files) may be absent. `fufu.keep` defaults to 90 days. Captures live under `refs/fufu/`, outside branch history and ordinary pushes.

**Argument position selects the address space.** Commit SHAs and operation IDs are hexadecimal; change IDs use k–z and survive fufu rewrites. Revision positions accept SHAs, change IDs, branches, tags, and revision sets. Operation positions accept retained operation IDs and expressions. Commit/change prefixes need four characters and a unique match; divergent copies require a SHA. Operation-prefix uniqueness includes trash and other worktrees.

Revision `@` is the open change, `@^` is HEAD, and `@~3` is HEAD~2. Operation `@` is the live tip, `@^` its predecessor, and `@~3` three back. `ff show` takes revisions; `ff op show` takes operations. `--at-op <id>` takes an operation; `--at` takes a time. Only restore and op log/show/diff implement past-state reads.

## Recovery

Inspect first: `ff history` lists undo steps, `ff evolog` lists the open change's captures, and `ff op log` lists recorded operations. Copy an actual hexadecimal operation ID when recovering a particular capture.

| Situation | Command |
| --- | --- |
| Take back the last undo step in this worktree | `ff undo` |
| Go forward again | `ff redo` |
| Restore this worktree's recorded local state at an operation | `ff op restore <id>` |
| Reverse an old operation's still-applicable ref transitions | `ff op revert <id>` |
| Discard a file's uncommitted edits | `ff restore <path>` |
| Recover a file from a retained capture | `ff restore <path> --at-op <id>` |
| Recover all eligible files from a retained state at a time | `ff restore --all --at 2h` |
| Copy a file from another revision | `ff restore <path> --from <rev>` |
| Inspect an operation's files and ref transitions | `ff op show -p <id>` |
| Compare files in two operation trees | `ff op diff <a> <b>` |

Undo restores recorded local refs, HEAD, index, and files, subject to worktree guards. Consecutive captures can collapse into one step; use a capture ID for precise file recovery. Undo cannot reverse a remote push or recover uncaptured bytes. Restore changes only worktree files, preserving refs, HEAD, and index. It captures first; `--at` accepts `30m`, `2h`, `3d`, or a date.

Undo and op restore move the log pointer. Redo walks forward until new work forks the log; the old branch of operation history remains addressable by ID until trimmed. `ff op trim` applies retention; it also invokes Git gc --auto even when nothing is dropped.

**Op revert only reverses ref transitions.** Every affected ref must still equal the value the operation left. It preserves files, index, and HEAD selection, and appends an undoable operation. `held/op-revert` refuses applicability without creating a resolution session; pre-capture/reconciliation can still write. Session tags do not isolate shared-worktree edits or undo chains.

## Reading

- `ff status` shows branch, open work, counts, pull comparisons, reconciled foreign changes, holds, and sessions.
- `ff diff` shows the open patch, eligible untracked files included.
- `ff log` shows change IDs in the letters column and SHAs separately; `--commits` omits change IDs. The open row's internal SHA can change at commit. Highlighted change prefixes are page-local and can be shorter than lookup accepts.
- `ff log -r <revset>` selects revisions. Positional arguments are paths: `ff log main` filters a file named main.
- `ff show` shows the open change with its patch; pass a revision to inspect another change.
- Bare `ff` shows recent work across branches, including parked work.
- `ff evolog <rev>` shows operations that produced that change and captures behind its commit; bare evolog shows the open change's captures.
- `ff collide <branch>` compares conflicts in memory; both verdicts exit 0. CLI capture, fetch, and maintenance can still run.

Use `ff -C <dir> status` for another worktree. Use bounded ranges such as HEAD~2..HEAD for this branch's last two commits; an omitted right endpoint can include other branches.

## Committing

Follow the project's commit-message convention.

- `ff commit -m "…"` records all eligible edits. A clean tree has nothing to commit.
- `ff commit <path> -m "…"` selects files or directory prefixes, without globs or hunks. Other work remains open without a pending description. Repeat to split work into commits.
- `ff describe -m "…"` sets the internal open commit's pending message; `ff commit` uses it without adding a message flag.
- `ff describe <rev> -m "…"` rewords a recorded commit and replays descendants.
- `ff commit -b <branch>` renames an automatically named branch, otherwise creates a branch here.
- `ff commit --no-verify` skips pre-commit and commit-msg hooks. Signing follows Git configuration; `-S` and `--no-sign` override it.

## Rewriting

`ff absorb` and `ff lift` move file changes between commits, including the open change. Paths select whole files, not hunks. `--from <revset>` selects a contiguous source run; `--into <rev>` names its target. Emptied sources are dropped; surviving changes retain IDs while SHAs change.

- `ff absorb` adds uncommitted changes to HEAD; `ff absorb --into <rev>` targets an earlier commit.
- `ff absorb --from HEAD~2..HEAD` combines the last two commits into their parent. A committed-source move whose implicit target lies on trunk is refused; name the intended target explicitly. Adding only uncommitted work to trunk's tip is allowed.
- `ff lift` moves HEAD's changes into the open change. `ff lift --from <rev>` selects another source; `--into <rev>` targets a recorded commit instead.
- `-m` gives the target a message, including a pending description when the target is open.
- `ff edit <rev>` creates and switches to an editing-session branch at a recorded commit. Edit and test its real files, then `ff done` amends it, replays waiting descendants, and returns. Prior open work parks and resumes on return.
- `ff done --abandon` removes the session without applying its edits. Captured edits remain in the operation log, not a stash. Opening an edit session takes two undo steps: return, then remove it. Landing or abandoning takes one.
- `ff restack` replays the current branch onto its recorded base. `ff restack <branch>` targets another branch; local files change only if the replay or cascade reaches this worktree.
- `ff restack --onto <branch>` records a new base and replays onto it; origin/main is accepted. `--no-fetch` skips auto-fetch.

Dependent local branches replay parent before child after a successful rewrite. The primary rewrite and its cascade form one undoable operation. A conflicting primary replay records a hold without landing that rewrite; captures and metadata can still be written. Earlier successful cascade updates stand.

**Inspect cascade reports even on exit 0.** Pull and restack exit 3 for holds; pull also predicts holds in dry runs. Done and absorb/lift exit 3 for a primary hold, but exit 0 after landing with downstream holds. Describe exits 0 with a held cascade. Fold's primary conflict exits 1 without a hold; downstream holds exit 3. Skips name branches checked out elsewhere, already held, or containing merges. Descendants of held or skipped branches stay put.

## Held rewrites and conflicts

A **held rewrite** is a replay waiting on conflicting changes. Status reports it; push blocks that branch. Switch to reach a hold elsewhere.

- `ff resolve` opens labeled markers on a session branch. The hold and prior open work remain on the original branch. Fix markers, then `ff done` lands and returns. Switching away parks fixes; switching back resumes them.
- If the rewrite now applies cleanly, resolve releases the hold instead. Re-run the command that requested the replay to apply it.
- `ff resolve --abandon` removes the hold and any open resolution session, returning from that session if needed.
- Opening a rewrite-resolution session is two operations: one undo returns from it, another removes it. Landing or abandoning is one undoable operation. Use history to account for any intervening work.
- A held **parked-change arrival** means switch could not replay parked work onto a moved tip. Resolve puts markers into the open change in place and removes the hold, in one operation. It refuses existing open work. Fix files and continue; there is no session or done step.

A hold leaves that branch's replay unapplied; earlier writes in the same command can stand. There is no Git rebase to continue. Restack skipped branches once their blocking condition clears.

## Branches and worktrees

- `ff switch <branch>` continues a local branch, parking work on departure and resuming the destination's saved open change. Parked work is stored with that branch, not in git stash list.
- `ff start` aliases switch. No target creates an automatically named branch at trunk; a revision creates one there; a remote-only name creates a tracking branch. `-b` forks a branch target or names the new branch. `ff switch @` copies open work onto a new branch. Switching records no commit in branch history.
- `ff describe -b <name>` renames the current branch with its fufu metadata.
- `ff branch` lists branches; `ff branch <name>` creates one at trunk by default; `ff branch -d <name>` removes one undoably.
- `ff worktree <path>` creates another checkout with its own branch, files, operation chain, undo, and capture lock; shared refs can contend.

## Remotes

- `ff pull` updates the current branch and its bases, parent before child. `ff pull <branch>...` selects branches; `ff pull --all` selects all local branches. Local changes are one undoable operation; fetched objects, tracking refs, and tags are separate.
- `ff pull -n` previews without moving local branches/files or recording holds. Fetch still writes objects, tracking refs, and tags; `--no-fetch` uses existing refs. Dry runs can still trim and run update maintenance.
- `ff push` sends the current branch; `ff push <branch>...` sends named branches. Each lease checks the expected remote ref value. Replacing commits also requires tracking-tip agreement with fufu's seen record; fast-forwards can proceed without it. Auto-fetch does not refresh that record. There is no push --all, branch-ownership check, or special main protection.
- Current off-branch push notes can replace a named target's open state with the current worktree's tree. Prefer switching to each branch and pushing it while current. If already affected, inspect retained pre-push captures before recovering parked edits.
- A multi-branch push refusal exits 1 even if other branches succeeded or were blocked by holds; holds alone exit 3. `ff push -n` previews without sending, but auto-fetch and maintenance can still run. `ff push --to <remote>` records the branch's remote selection.
- Pushed commits can be rewritten; team policy and server protections govern sending them. Remote rollback requires another permitted push with a valid lease.

## Git policy and state ownership

Only fufu writes `refs/fufu/*`; never hand-edit them. Extensions read state and call fufu verbs.

`fufu.gitPolicy` has observe (record only), coach (default, suggest equivalents), and strict (refuse mapped writes). Set it with `ff config gitPolicy <tier>`. `ff git <args…>` attempts capture before permitted Git commands; a strict refusal exits 2 before capture or execution, but records the policy tally. Capture failure on an allowed passthrough warns and Git still runs.

Active agent hooks attempt capture before policy evaluation. Only the current Claude Code adapter emits pre-tool coaching or denial; the client must enforce the denial. Codex, Cursor, and Gemini adapters record the tally but emit no tool reply. Unmapped commands and ambiguous shell strings remain allowed. Hooks must be active; Claude Code needs a restart after installation, and Codex requires /hooks approval of new or changed hooks.

## JSON output and scripting

Reporting commands use a one-line envelope: `{"ff":1,"cmd":"status","data":{}}` illustrates its shape. Failures replace data with error (id, message, exits). Read the exit code too: data can accompany nonzero exits and partial success. Contract 1 is current, not a cross-release payload/error-ID guarantee. Pin/test binary versions, assert the envelope version, and tolerate unknown fields.

Git passthrough uses Git's streams/status; update prints instructions or installer output; client triggers use client protocols and quietly exit 0 on runtime failure; extensions own their output. `ff watch` always emits newline-delimited event envelopes rather than one command report. Do not assume --json makes these uniform.

- Exit 1 means failure or a negative check; 2 is usage; 3 is held/blocked; 4 is ref/contended. Bound contention retries, inspect partial results, and do not retry holds blindly. Cascade exit-0 exceptions are above.
- Set FF_NONINTERACTIVE=1 and supply messages/answers through flags. Built-in prompts and editors are disabled with nonterminal stdin; Git, extensions, hooks, and external tools keep their own interaction rules. Piped output and JSON do not page.
- `ff op log` polls retained records. `ff watch` streams them in the foreground; `--all` follows all worktrees. Watch's --session filters events; the global --session on other commands tags newly recorded operations. Filter op log with its session() expression, not that flag. FF_SESSION is the environment tag, followed by CLAUDE_CODE_SESSION_ID as fallback; a valid hook payload session takes precedence for that capture.
- Operations are write-ahead records, not proof that the described mutation finished. A watch chain rewrite emits rewritten and exits 1 in single-worktree mode; --all re-anchors that chain and continues. Reconnect or refresh affected anchors.
- `ff explain <id>` explains refusals; `ff explain --list` lists them. `ff config` reports settings. `ff doctor` exits 1 on findings; capture/reconciliation, fetch, and maintenance can run. `--fix` repairs supported gc, branch-config, hook, and skill findings.
- Unknown verbs run ff-<name> from PATH when available. Extensions inherit the environment with FF_REPO, FF_CONTRACT, and FF_SESSION supplied by fufu. They own their arguments, output, and exit codes.

## The authority

Every verb's `--help` is the authority. The docs' Revisions and IDs reference owns expressions; the recovery guide provides scratch recipes.
