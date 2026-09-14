# Stacked changes

A stack splits a feature into reviewable branches. Each branch records the branch beneath it as its base, so feedback on a lower layer can replay the work above it.

```text
main
  └── parser-core       parser implementation
        └── parser-cli  command that depends on the parser
```

| Task | Command | Next step |
| --- | --- | --- |
| Create the bottom branch | [`ff switch main -b parser-core`](../reference/cli/switch.md) | Write and commit its implementation. |
| Create a dependent branch | `ff switch parser-core -b parser-cli` | Write and commit the next layer. |
| Apply review feedback | [`ff absorb`](../reference/cli/absorb.md) on the lower branch | Inspect the dependent-branch report. |
| Take in remote updates | [`ff pull`](../reference/cli/pull.md) | Review and test the replayed stack. |
| Publish selected branches | [`ff push parser-core parser-cli`](../reference/cli/push.md) | Open or update each review. |
| Move remaining work after a merge | [`ff restack parser-cli --onto main`](../reference/cli/restack.md) | Inspect and push the remaining branch. |

The stack walkthrough starts in an initialized scratch repository with main, `app.txt`, and `README.md`. Its local `origin.git` and plain-Git `../teammate` checkout make remote steps reproducible without a service. `bash scripts/docs/stacked-changes-transcript.sh stack` supplies this setup and runs the complete stack lifecycle; `collision` runs the independent overlap recipe. The unused fixture branch `feature` is only used by the latter recipe.

## Start a stack

Prerequisite: main is the desired base and neither stack branch exists. A branch name alone resumes it; `-b` creates the named new branch. [`ff commit`](../reference/cli/commit.md) records each layer without staging.

<!-- transcript:stack -->
```console
$ ff switch main -b parser-core
minted parser-core (forked from main)
switched to parser-core
undo: ff undo

$ printf 'parser core\n' > parser.txt

$ ff commit -m "parser: core"
closed 6a773795 on parser-core: parser: core (1 file(s))
undo: ff undo

$ ff switch parser-core -b parser-cli
minted parser-cli (forked from parser-core)
switched to parser-cli
undo: ff undo

$ printf 'parser command\n' > cli.txt

$ ff commit -m "cli: parser command"
closed ff5e780e on parser-cli: cli: parser command (1 file(s))
undo: ff undo

$ ff log
@  no changes
│  (no description)
●  ullormnz ff5e780e   0s ago
│  cli: parser command
●  vqmwrqkn 6a773795   0s ago
│  parser: core
●  okzskrsy 4aacb104   0s ago
│  demo: initial files

```
<!-- /transcript -->

The recorded base of parser-cli is parser-core. Keep implementation and CLI feedback on their respective branches; they can now be reviewed separately.

## Read the stack in the map

[`ff log`](../reference/cli/log.md) above shows the current ancestry, with a stable change ID before each commit hash. [`ff map`](../reference/cli/map.md), also available as bare `ff`, adds branch markers so you can see where each layer ends. `@` is current open work; `~` elides commits. A fork can indicate an unrelated branch or a dependent branch that has not followed its base; use the command report to tell which.

## Land review feedback with absorb

Prerequisite: parser-cli is based on parser-core as above. Switch to the lower branch and write its feedback there. Bare absorb amends its latest commit; `--into <revision>` selects an earlier commit on that branch.

<!-- transcript:feedback -->
```console
$ ff switch parser-core
switched to parser-core
undo: ff undo

$ printf 'reviewed parser core\n' > parser.txt

$ ff absorb
moved 1 file(s) from the open change into c1b8a2ef: parser: core
parser-cli followed parser-core: replayed 1 commit(s)
undo: ff undo

$ ff log -r parser-cli
●  ullormnz 588987d3   0s ago
│  cli: parser command

```
<!-- /transcript -->

Parser-core now contains the reviewed implementation. The `parser-cli followed parser-core` line confirms the CLI commit replayed onto it. Inspect both layers and run their tests before continuing.

## The cascade

Restack, pull, absorb, [`ff lift`](../reference/cli/lift.md), [`ff describe <revision>`](../reference/cli/describe.md), and [`ff done`](../reference/cli/done.md) replay dependent branches parent before child when they move a base tip. This automatic replay is the cascade. Successful updates belong to the initiating operation, so one [`ff undo`](../reference/cli/undo.md) reverses that operation and its cascade.

A downstream conflict leaves that branch's tip in place and records a held rewrite. Earlier successful updates remain; branches depending on the held branch stay put. Branches checked out in another worktree, already held, or containing merges are skipped and named. A branch inside the replay range with no commits of its own can also stay put and be reported. Resolve the reported condition, then restack the branch onto its recorded base.

| Verb outcome | Exit behavior |
| --- | --- |
| Restack or pull has a primary or downstream hold | 3; read which other branches already updated. |
| Absorb or lift has a primary hold | 3. |
| Describe, absorb, or lift succeeds but a downstream branch holds | 0; inspect the cascade report or JSON. |
| Done cannot complete its primary replay | 3; the session needs attention. |
| Done lands successfully but a downstream branch holds | 0; the downstream hold is still reported. |
| A cascade only skips branches | Skips do not by themselves imply exit 3. |

Switch to a held branch and use [`ff resolve`](../reference/cli/resolve.md), fix the marked files, then done. Its dependents can follow once it lands. [Conflicts and held rewrites](../concepts/held-rewrites.md#reading-conflict-reports) explains sessions, parked arrivals, and machine reports. Do not infer from one nonzero exit that every branch and record stayed untouched.

## Pull the stack

Prerequisite: parser-core is current, main tracks origin/main, and parser-cli depends on parser-core. The first four commands model a teammate adding an independent file on main.

<!-- transcript:pull-stack -->
```console
$ printf 'teammate documentation\n' > ../teammate/notes.txt

$ git -C ../teammate add notes.txt

$ git -C ../teammate commit -qm "docs: teammate notes"

$ git -C ../teammate push -q origin main

$ ff pull
fetching from origin
main moved ahead by 1 commit(s)
replayed 1 commit(s) onto main
parser-cli followed parser-core: replayed 1 commit(s)
updated the working copy (1 file(s))
not published yet — ff push
main
    fast-forwarded to origin/main (1 commit(s))
undo: ff undo

```
<!-- /transcript -->

Pull fetches and updates the selected branch with its base and remote copy. Here it brings main level with origin/main, replays parser-core, and cascades to parser-cli. The local branch and file updates are one undoable operation; fetched objects and tracking refs are separate. `--no-fetch` uses already available remote information, while `--dry-run` still fetches unless combined with it. Use `ff pull --all` when you intend to select all local branches. Inspect and test the resulting files before pushing.

## Push each branch under its own lease

Prerequisite: both stack branches are ready to publish. This example switches to main first to make the explicit selection visible: only parser-core and parser-cli are sent, not the current branch or an automatically expanded set of ancestors.

<!-- transcript:push-stack -->
```console
$ ff switch main
switched to main
undo: ff undo

$ ff push parser-core parser-cli
parser-cli
    created origin/parser-cli and set parser-cli to track it
parser-core
    created origin/parser-core and set parser-core to track it
the push left the machine — ff undo cannot reach it
ff undo then ff push rolls the shared copy back, under a lease

```
<!-- /transcript -->

Pushing parser-cli alone transfers the Git objects needed for its history, including ancestor commits, but does not create or update the remote parser-core ref. Name both branches to publish both review targets. Each has its own lease and result; a refusal on one does not roll back successes on others. Read every result block, pull the refused branch, inspect the replay, and retry as appropriate. [Pulling and pushing](../concepts/push-boundary.md) explains the seen-record check and remote rollback limits.

### Inspect local work after a named push

Named pushes preserve each target's saved working state. This fixture had no uncommitted work on either target, so switching to parser-cli shows its committed files and a clean working copy. Genuine parked edits resume when you switch to their branch.

In v0.16.0, an off-branch push could instead save the current checkout's tree as the target's open work. Switching there could resume apparent deletions or overwrite parked edits. Updating prevents new occurrences but does not reconstruct already affected work. If the branch had no uncommitted edits, [`ff restore --all`](../reference/cli/restore.md) restores its committed files. To recover genuine parked edits, use a retained pre-push capture; [file recovery](recovery.md#restore-files-by-time-or-snapshot) explains how. While using an affected version, push each branch while it is current.

<!-- transcript:after-named-push -->
```console
$ ff switch parser-cli
switched to parser-cli
undo: ff undo

$ ff status
on parser-cli · nothing to pull
@  no changes
│  (no description)
●  ullormnz 196a010c   1s ago
│  cli: parser command

$ ff switch parser-core
switched to parser-core
undo: ff undo

$ ff switch main
switched to main
undo: ff undo

```
<!-- /transcript -->

## When the bottom lands

Prerequisite: parser-core has merged into main. This recipe models a fast-forward merge in the teammate checkout, then updates local main before changing parser-cli's base.

<!-- transcript:merged -->
```console
$ git -C ../teammate fetch -q origin

$ git -C ../teammate merge -q --ff-only origin/parser-core

$ git -C ../teammate push -q origin main

$ ff pull main
fetching from origin
fast-forwarded to origin/main (1 commit(s))
updated the working copy (1 file(s))
undo: ff undo

$ ff restack parser-cli --onto main
re-aimed parser-cli at main (was parser-core)
undo: ff undo

$ ff push parser-cli
parser-cli
    nothing to push

```
<!-- /transcript -->

Parser-cli now records main as its base, with only its CLI commit ahead. This fast-forward merge preserved its existing ancestry, so there was no new commit to send: the final push reports nothing to push. Inspect the remaining diff and update its review target on the forge. Squash and rebase merges can change the identity or ancestry used to recognize already-landed work; inspect the replay report and remaining commits for your team's merge style before pushing. Branch cleanup is separate, through [`ff branch`](../reference/cli/branch.md).

## Would two branches collide

Prerequisite: two independently based branches touch the same file. This standalone recipe starts with the common fixture, writes one greeting on feature, then a conflicting greeting plus independent notes on renamer.

<!-- transcript:collision -->
```console
$ printf 'parser greeting\n' > app.txt

$ ff commit -m "app: parser greeting"
closed aa28d305 on feature: app: parser greeting (1 file(s))
undo: ff undo

$ ff switch main -b renamer
minted renamer (forked from main)
switched to renamer
undo: ff undo

$ printf 'renamed greeting\n' > app.txt

$ printf 'rename notes\n' > notes.txt

$ ff commit -m "app: rename greeting and add notes"
closed 11104b93 on renamer: app: rename greeting and add notes (2 file(s))
undo: ff undo

$ ff collide feature
  renamer  ✕ feature  app.txt

$ ff lift app.txt
moved 1 file(s) from 11104b93 "app: rename greeting and add notes" into the open change
limited to 1 path(s)
undo: ff undo

$ ff collide feature
  renamer*  ✕ feature  app.txt

  * has uncommitted work

$ ff restore app.txt
restored from fc6eff41 (app: rename greeting and add notes)
  restored  app.txt
undo: ff undo

$ ff collide feature
  renamer  ✓ feature

```
<!-- /transcript -->

[`ff collide`](../reference/cli/collide.md) with one branch compares it with the current branch. It compares recorded trees, including captured uncommitted work. Lifting app.txt alone would still leave a collision because that edit is now open; restoring it removes the conflicting edit while keeping the committed notes. Review the kept work before continuing. The source script also verifies the intermediate dirty collision through JSON.

Collision findings exit 0 whether the branches conflict or merge cleanly; scripts must inspect the verdict. The comparison itself leaves branches, index, and files alone, but capture, auto-fetch, and maintenance can run around it. For an already-held replay, use resolve rather than collide.

## From here

- [Rewriting history](rewriting-history.md) — amend, reword, split, combine, and edit individual commits.
- [Worktrees](worktrees.md) — work on separate branches in simultaneous checkouts.
- [Recovery](recovery.md) — recover local state after a mistaken operation.
