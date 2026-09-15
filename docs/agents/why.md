# Why agents want fufu

An agent can overwrite a useful experiment before anyone commits it. fufu records working-copy snapshots through repository commands and active hooks, so you can inspect and recover intermediate file states as well as branch history. [Install the agent hook](setup.md) to start recording the client's supported tool events.

## The failure mode

Suppose an agent replaces a parser, runs tests, then discards the replacement after misreading a failure. The last commit still exists, but the useful uncommitted version may not. Git's reflog records ref movement; it does not preserve each intermediate working-copy tree. A commit, stash, editor history, or another backup may help if one contains the missing content.

Agents can checkpoint deliberately, but those checkpoints depend on when they choose to save. Automatic snapshots reduce that dependency, especially during rapid edit-and-test cycles.

<a id="the-net"></a>
## Recover the useful version

With an active hook, a snapshot before the discard can retain the replacement. [`ff evolog`](../reference/cli/evolog.md) shows captures of the open change. [`ff op show`](../reference/cli/op-show.md) inspects one record, and [`ff restore <path> --at-op <id>`](../reference/cli/restore.md) brings back the file without moving branch refs or changing the index.

For a broader mistake, [`ff history`](../reference/cli/history.md) shows undo steps and [`ff undo`](../reference/cli/undo.md) restores recorded local refs, HEAD, index, and files together. Consecutive captures can collapse into one step; a session can span several steps. Inspect the map before deciding how far to go. The [recovery guide](../guides/recovery.md) gives independent recipes for both scopes.

Recovery needs a successful, retained capture. Hooks cover the events they receive, and ignored untracked files, oversized content, and unsaved editor buffers can be absent. See [snapshot timing and limits](../concepts/snapshots-and-undo.md#coverage-and-limits). [`ff trigger -m "checkpoint"`](../reference/cli/trigger.md) also lets an agent label a manual snapshot.

<a id="the-net-covers-git-itself"></a>
## Capture around Git commands

[`ff git <args…>`](../reference/cli/git.md) attempts a snapshot before running a permitted Git command verbatim. The [shell alias](../concepts/two-regimes.md#which-program-ran) routes shell invocations through that passthrough when active; agent hooks independently capture supported tool calls.

A permitted `reset --hard` still resets. If the preceding capture retained the discarded edits, fufu can restore them. Outside ref changes are reconciled into foreign-operation records on the next capture, but reconciliation cannot recreate file bytes that were never captured.

<a id="the-human-keeps-the-last-word"></a>
## Review an agent's work

Use the same history to accept useful work and recover from mistakes:

- `ff history` gives the coarse view: one row per undo step.
- [`ff op diff`](../reference/cli/op-diff.md) compares files in two operation trees; `ff op show` includes ref transitions.
- [`ff op log 'session(<id>)'`](../reference/cli/op-log.md) selects records tagged with one agent's session. Tags identify work while the records are retained; they do not create separate undo histories in a shared worktree.

[`ff commit`](../reference/cli/commit.md) records eligible work in branch history. File recovery leaves other work in place; whole-state undo has a wider effect. A push changes the remote separately and cannot be undone locally.

## A surface with fewer ways to go wrong

An agent using fufu does not need to maintain a staging area or remember a stash before switching branches:

- `ff commit -m "…"` records eligible working-copy changes. Paths select a partial commit at commit time; review the patch and capture warnings first.
- [`ff switch <branch>`](../reference/cli/switch.md) parks open work with the branch being left and resumes work saved at the destination.
- A conflicting rewrite is recorded as a hold on the affected branch. [`ff resolve`](../reference/cli/resolve.md) opens the conflicts for editing. Earlier successful updates in the same run can still stand.

For automation, [JSON output and scripting](machine-surface.md) explains structured reports, exit codes, noninteractive flags, and command-specific stream contracts. A script checks both the report and the exit code, using a tested binary version.

<a id="the-supervisor-pattern"></a>
## Supervise separate writers

Separate [worktrees](../guides/worktrees.md#two-writers-one-repository) give agents independent files and undo chains while sharing repository history. In one shared worktree, an undo can include another writer's later work. Review the records and use [scoped recovery](../guides/recovery.md#two-writers-on-one-chain-and-only-one-was-wrong) where applicable.

<a id="strict-mode-as-a-leash"></a>
## Guide Git usage with policy

The default `fufu.gitPolicy=coach` suggests fufu equivalents for recognized Git writes. `strict` refuses mapped writes through `ff git` and requests denial through the Claude Code hook. The current Codex, Cursor, and Qwen Code adapters emit no pre-tool policy reply. [Agent setup](setup.md#pick-a-git-policy) explains the client differences and refusal timing.

Unmapped commands and ambiguous shell strings remain allowed. Policy guides version-control commands; it does not isolate an agent's process or replace snapshot verification.

## What this rests on

Snapshots are ordinary Git objects under fufu's refs, separate from commits recorded in branch history. The [Git storage model](../concepts/invariant.md) explains the representation; [Using fufu alongside Git](../concepts/two-regimes.md) covers practical compatibility. Start with [setup and a recovery check](setup.md#verify) to see what your client actually records.
