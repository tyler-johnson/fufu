# fufu vs git

fufu replaces staging, manual stashing, and common history-editing sequences with commands that record local recovery state. It uses ordinary Git objects and branches, so teammates can keep using Git. The main costs are learning different command defaults, relying on snapshot coverage, and giving up a native hunk picker.

Use the [command table](command-table.md) to translate a task. [Using fufu alongside Git](../concepts/two-regimes.md) covers practical compatibility. The Git behaviors compared here are documented in the [Git 2.50.1 manuals](https://github.com/git/git/tree/v2.50.1/Documentation).

## What disappears

- **Staging before a fufu commit.** [`ff commit`](../reference/cli/commit.md) records the working copy. Path arguments select files or directories at commit time; other edits stay open. Git's index still exists, but a staged selection does not control a fufu commit. See [partial commits](../concepts/changes.md#partial-commits).
- **Manual stashing when switching through fufu.** [`ff switch`](../reference/cli/switch.md) parks current work with its branch and resumes the target's work. This includes eligible untracked files, subject to [capture limits](../concepts/snapshots-and-undo.md#coverage-and-limits). Staged distinctions become ordinary working-copy edits on return.
- **Interactive-rebase instructions for common edits.** [`ff describe <rev>`](../reference/cli/describe.md) rewords; [`ff absorb`](../reference/cli/absorb.md) moves content into a commit; [`ff edit <rev>`](../reference/cli/edit.md) opens a session branch and [`ff done`](../reference/cli/done.md) lands it. Descendants replay automatically, subject to [cascade skips and holds](../concepts/branches.md#the-cascade).
- **Separate base and remote reconciliation commands.** [`ff pull`](../reference/cli/pull.md) fetches and updates the selected branches. [`ff restack`](../reference/cli/restack.md) replays onto the recorded base and can auto-fetch first; `--no-fetch` disables that fetch.
- **Reconstructing routine recovery from individual reflogs.** [`ff history`](../reference/cli/history.md) lists undo steps; [`ff undo`](../reference/cli/undo.md) restores recorded local state along the current worktree's log. Git's reflogs remain available for history outside that log.

<span id="what-your-aliases-cannot-do"></span>

## What fufu records for you

Aliases and scripts can automate Git commands, including snapshots. fufu supplies a shared operation record and recovery interface across its commands and installed hooks, so you do not have to maintain that integration yourself.

<span id="it-cannot-act-before-you-type"></span>

### Capture timing

Repository readers normally attempt capture; mutators capture after initial guards. Active shell and agent hooks add capture opportunities at their supported events. [`ff trigger -m "before refactor"`](../reference/cli/trigger.md) requests a manual snapshot. None of these can recover bytes that were never captured or are no longer retained. See [when snapshots run](../concepts/snapshots-and-undo.md#when-snapshots-run).

<span id="it-cannot-give-one-account-of-what-happened"></span>

### One recovery log per worktree

The operation log records captured files and local operations. On returning after outside Git changes, fufu reconciles observed ref movements into a foreign operation. It cannot reconstruct intermediate file contents that no capture saw. [Returning after outside changes](../concepts/two-regimes.md#returning-after-outside-changes) explains this boundary.

## The opinions, and where they stop

fufu's update commands replay feature work onto its base rather than merging the base into it. Rewriting commits is routine, including commits already pushed. [`ff push`](../reference/cli/push.md) is a separate explicit command; a lease checks the expected remote tip, and replacing commits also requires fufu's seen record to agree with the tracking tip.

These checks do not establish who owns a branch or make published history append-only. Teams choose their [shared-history policy](../concepts/push-boundary.md#shared-history-policy) and server-side protections. A clean textual replay still needs the project's normal verification.

## What stays git

- **Storage and tooling.** Commits, refs, configuration, and worktrees use Git formats. fufu adds refs and metadata; tools that list all refs can see them. [Git storage model](../concepts/invariant.md) explains what those records mean.
- **Remotes and forges.** Branch updates use Git's protocol and obey the server's rules. Native clone/fetch and Git-backed push have different [transport dependencies](../internals/substrate.md#the-execution-ladder-as-it-stands).
- **Commit hooks.** fufu runs the supported commit hooks itself. Which hooks run depends on the operation and options; see the [hook table](../faq.md#does-fufu-run-my-git-hooks).
- **Team landing workflow.** Merge queues, squash buttons, and review rules stay with the team and forge.
- **Other commands.** [`ff git <args>`](../reference/cli/git.md) runs Git for merge, cherry-pick, bisect, and plumbing. LFS and submodules remain [unsupported or untested](../faq.md#does-fufu-work-with-git-lfs); passthrough availability is not a compatibility guarantee.

## The honest costs

- **Different selection model.** fufu selects paths, not hunks. Git's interactive commit remains available under `coach` or `observe` policy, but mixing a hand-staged index with a subsequent fufu commit does not work.
- **Different defaults.** Bare switch starts at trunk; restore changes files without resetting the index or branch. The command table gives these qualifications beside each task.
- **Capture and retention need attention.** Hooks must be installed and active. Ignored untracked files, oversized capture content, and unsaved buffers have [coverage limits](../concepts/snapshots-and-undo.md#coverage-and-limits). [`ff doctor`](../reference/cli/doctor.md) checks configuration and records, not every live client's activation.
- **Local records consume storage and time.** File scanning grows with the working tree. Recovery refs retain objects until retention releases them; [performance](../performance.md) states the scope of the existing measurements.
- **Recovery has a boundary.** Removing the binary leaves Git usable. Deleting fufu's recovery refs or metadata can lose access to parked work and past states, and later garbage collection can delete unreferenced objects. See [leaving and coming back](../concepts/two-regimes.md#leaving-and-coming-back).
