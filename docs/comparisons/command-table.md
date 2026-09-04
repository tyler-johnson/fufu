# Command table

Every row maps a git habit to the fufu verb that replaces it. One difference is shared by all of them and not repeated below: every fufu verb captures the working tree before it acts, and lands on the operation log where [`ff undo`](../reference/cli/undo.md) can take it back.

Where fufu has no verb, [`ff git <args>`](../reference/cli/git.md) runs your git command verbatim after the snapshot, so no habit is left without a home. [The two regimes](../concepts/two-regimes.md) is the full account of that boundary.

Almost no mapping is exact, because a fufu verb exists only where it does something git's version does not. The numbered notes after the table say where each row's two sides part; the vocabulary they use — the open [change](../concepts/changes.md) you are in the middle of, parking, the [lease](../concepts/push-boundary.md) that guards a push, [trunk](../concepts/glossary.md) as your main line of development, minted names, [held rewrites](../concepts/held-rewrites.md) — is defined in the concepts pages.

| you'd type in git | in fufu | the difference |
| --- | --- | --- |
| `git init` | [`ff init`](../reference/cli/init.md) | armed before your first commit: the gc guard and the log's floor ¹ |
| `git clone` | [`ff clone`](../reference/cli/clone.md) | armed on arrival; fufu speaks the protocol itself ² |
| `git status` | [`ff status`](../reference/cli/status.md) | futures included: what sync would do, and any held rewrite ³ |
| `git diff` | [`ff diff`](../reference/cli/diff.md) | sees untracked files, with their content ⁴ |
| `git log` | [`ff log`](../reference/cli/log.md) | the open change is a row; operation ids attached ⁵ |
| `git log --follow -p -- <file>` | `ff log <file>` | no `--`; renames followed by default ⁶ |
| `git show` | [`ff show`](../reference/cli/show.md) | bare shows the open change; one renderer with `ff diff` ⁷ |
| `git branch -v` + `git stash list` + remembering | bare `ff` ([`ff map`](../reference/cli/map.md)) | the map: recent work across every branch, parked changes included ⁸ |
| `git add` + `git commit -m` | [`ff commit -m`](../reference/cli/commit.md) | no staging; the tree is the change ⁹ |
| `git add -p` + `git commit` | `ff commit <paths>` | a slice: selection at the moment of the close, not a staging area ¹⁰ |
| `git checkout -b` | [`ff start`](../reference/cli/start.md) | always forks from trunk; the name can come later ¹¹ |
| `git switch` + the stash dance | [`ff switch`](../reference/cli/switch.md) | parking is automatic, per branch ¹² |
| `git stash` + `git stash pop` | `ff switch` away, and back | nothing to remember to pop; the park rides the branch ¹³ |
| `git commit --amend --no-edit` / `fixup!` + autosquash | [`ff absorb`](../reference/cli/absorb.md) | folds and restacks above the target in one operation ¹⁴ |
| `git commit --amend -m` / `rebase -i`, `reword` | [`ff describe <rev> -m`](../reference/cli/describe.md) | one verb, automatic restack ¹⁵ |
| `git rebase -i`, `edit` | [`ff edit <rev>`](../reference/cli/edit.md) … [`ff done`](../reference/cli/done.md) | a real branch, your whole toolchain; one operation to land ¹⁶ |
| `git reset --soft HEAD~` | [`ff lift`](../reference/cli/lift.md) | contents return to the open change; an emptied commit is dropped ¹⁷ |
| `git reset --hard` | [`ff restore --all`](../reference/cli/restore.md) | worktree only; refs never move by hash ¹⁸ |
| `git restore <path>` / `git checkout <rev> -- <path>` | `ff restore <path>` / `ff restore --from <rev> <path>` | one verb for every source: revision, operation, time ¹⁹ |
| `git rebase` | [`ff restack`](../reference/cli/restack.md) | replays in memory; lands only if clean ²⁰ |
| `git rebase --onto <base>` | `ff restack --onto <base>` | records the new base — this is how a branch is re-aimed ²¹ |
| the `git rebase --continue` loop | [`ff resolve`](../reference/cli/resolve.md) … `ff done` | all conflicts at once, on your schedule ²² |
| `git fetch` + `git rebase origin/main`, `git pull --rebase` | [`ff sync`](../reference/cli/sync.md) | one replay for base and remote; nothing leaves the machine ²³ |
| `git push` / `--force-with-lease` / `-u` | [`ff publish`](../reference/cli/publish.md) | leased; the four push shapes distinguished by `--dry-run` ²⁴ |
| `git branch` | [`ff branch list`](../reference/cli/branch-list.md) | named and anonymous kept apart; remote-only branches follow ²⁵ |
| `git branch -m` | `ff describe -b` | the rename carries everything the branch owns ²⁶ |
| `git branch -d` / `-D` | [`ff branch delete`](../reference/cli/branch-delete.md) | trash, undoable; no merged-check to argue with ²⁷ |
| `git worktree add` | [`ff worktree add`](../reference/cli/worktree-add.md) | undo works there from the first command ²⁸ |
| `git remote -v` | [`ff remote`](../reference/cli/remote.md) | a read; fufu's own verbs check names against it ²⁹ |
| `git reflog` + archaeology | [`ff history`](../reference/cli/history.md), `ff undo` | whole-repo: refs and tree together ³⁰ |
| `git cherry-pick` / `git merge` / `git revert` | `ff git cherry-pick` … | no fufu verb; snapshot first, then git verbatim ³¹ |
| anything else | `ff git <args>` | snapshot first, then git verbatim ³² |

## Notes

- ¹ Run inside a repository that already exists, `ff init` means turn fufu on here — the way to adopt a repository git created, or one cloned before fufu was on the machine.
- ² `ff clone` negotiates the pack itself rather than running `git clone`, while inheriting git's configuration and credential surface whole; because the clone arrives armed, `ff undo` works from the first command.
- ³ The files are a diffstat, not content — `ff diff` is the same change read down to the line — and status also reports what syncing would cost against the base and the remote copy, any held rewrite, and any work done behind fufu's back, which stays loud until absorbed into the operation log.
- ⁴ `ff diff` is the open change only, and its body is git's unified diff, so `git apply` reads it back; comparing two revisions stays `ff git diff`, and comparing the worktrees two operations carry is [`ff op diff`](../reference/cli/op-diff.md).
- ⁵ `-r` takes gitrevisions' whole grammar plus a set algebra, `--commits` drops to plain history, and each commit wears the id of its newest operation — the column [`ff evolog`](../reference/cli/evolog.md) drills into.
- ⁶ Positional arguments are only ever paths — `ff log main` asks about the path `main` even where that branch exists, and revisions go to `-r` — so the `--` disambiguator has nothing to do; a file is followed through renames unless `-r` narrows the set.
- ⁷ `ff show <op>` is refused toward [`ff op show`](../reference/cli/op-show.md), since letters-spelled ids are operations and hex ids are commits, and blobs stay git's: `ff git show HEAD:file.txt`.
- ⁸ The map draws only the commits that relate the branches shown and contracts the runs between them; parked changes appear on it, which is what makes it the `git stash list` replacement too.
- ⁹ There is no staging area to add into: closing the tree is the commit, `-m` wins over the pending description left by `ff describe`, and `-b` lands the close on a branch — claiming the anonymous one underfoot, or forking a fresh one from here.
- ¹⁰ Paths close a slice — a file or a directory prefix, no globs and no hunks — chosen once at the moment of the close rather than maintained in an index; the rest stays open, still the change you are in the middle of. `ff git commit -p` is git's own hunk picker over the worktree, capture-first, and `fufu.gitPolicy strict` refuses it with every other `git commit`.
- ¹¹ `ff start` forks from trunk rather than from where you stand — `ff start <rev>` forks elsewhere — and never creates a commit; bare, it mints an anonymous branch under a petname, `-b` names it at birth, and `ff new` is an alias.
- ¹² The open change parks with the branch you leave and whatever was parked at the target resumes — same files, same edits, same pending description; the target can be any unique prefix of a branch name, and one `ff undo` rolls the park and the move back together.
- ¹³ A park is an ordinary labeled `git stash` entry, visible to every git tool; to shelve work without a destination branch, `ff start` parks the open change and opens a clean one.
- ¹⁴ `ff absorb` folds the open change into the commit beneath it — `--into <rev>` reaches deeper — and everything above the target re-parents in the same operation; the change is the unit, so paths select files and there is no hunk attribution.
- ¹⁵ Naming a revision rewords a commit that has closed and restacks above it; bare `ff describe` edits the open change's pending description — a commit message before there is a commit, which git has no place for.
- ¹⁶ There is no detached HEAD: `ff edit` mints a branch at the commit and switches to it, and `ff done` amends, replays what waited ahead, and lands back as one operation, so one `ff undo` takes the whole session back; a replay that would conflict stops with nothing changed.
- ¹⁷ `ff lift` takes whole files out of the commit under the change — `--from <rev>` reaches deeper — back into the open change, restacking what sat above; a commit lifted empty is dropped.
- ¹⁸ `ff restore --all` writes the worktree alone — index, HEAD, and branches stay put — and deletes files created since, so it is closer to `git reset --hard` plus `git clean -fd`; moving a branch pointer back is not a restore but an undo of the operation that moved it, `ff undo` or [`ff op restore`](../reference/cli/op-restore.md).
- ¹⁹ Two more sources join `--from <rev>`: `--at-op <op>` reads from an operation and `--at <time>` from the operation current at that time; restore takes a mandatory capture first, so any restore is undone by another restore, or by `ff undo`.
- ²⁰ The base is the branch's recorded parent, trunk when none was recorded; the positional restacks a branch you are not standing on without touching a file on disk, and a conflict stops the run as a held rewrite rather than a mid-rebase worktree.
- ²¹ `--onto` records the new base before replaying, so the next bare `ff restack` needs no flag; a base on a remote, `origin/main`, records like any other.
- ²² A held rewrite blocks only `ff publish` and nothing local; `ff resolve` materializes every surviving conflict at once as labeled markers, `ff done` lands the rewrite, `--abandon` is the counterpart of `git rebase --abort`, and one `ff undo` takes the session back, markers and all.
- ²³ `ff sync` fetches, takes in what arrived from the base and from the remote copy, and replays your commits onto the result — landing only if clean, holding otherwise — and never pushes; there is no standalone fetch verb (`--no-fetch` skips the fetch, and a bare fetch is `ff git fetch`).
- ²⁴ Every publish carries a lease — it goes through only if the shared copy still stands where you last saw it — and `--dry-run` says which of the four pushes it would be: creating the shared copy, replacing it, putting back a deleted one, or rolling one back; `--to <remote>` records which remote the branch answers to, standing in for `-u`.
- ²⁵ Each row carries the tip, any parked change, the pending description, and how the branch stands against its upstream; a remote-only branch becomes a local one with `ff start origin/<name>`, not with switch.
- ²⁶ There is no separate rename command: `ff describe -b` names the branch you are on — a petname earning a real name, or a chosen name replaced — and the capture chain, any parked change, and the pending description come along, the parts a bare `git branch -m` would orphan.
- ²⁷ The branch's pointer moves to trash and `ff undo` brings it back with its timeline, so no merged-check argues with you; the copy on the remote stays unless you pass `--shared`, which deletes it under a lease — the half undo cannot reach.
- ²⁸ The chain floor is laid as the worktree is made; the branch defaults to one named after the directory, and a branch open in another worktree is refused rather than checked out twice.
- ²⁹ Adding a remote is still git's: `ff git remote add <name> <url>`.
- ³⁰ `ff history` shows one row per undo step, a run of captures collapsed into the keystroke it undoes as, with the redo path above; [`ff op log`](../reference/cli/op-log.md) is the other question — everything that happened, not just where you can go back to — and `ff undo` restores refs and tree together.
- ³¹ fufu has no verb for these, so they run through the passthrough, snapshotted and then verbatim; note that [`ff op revert`](../reference/cli/op-revert.md) inverts an operation on the log, not a commit, so `git revert` remains the way to invert a commit.
- ³² Nothing is ever translated — the command that runs is the one you typed — and `fufu.gitPolicy` decides only what fufu says about a git word it has a verb for: observe stays quiet, coach (the default) names the fufu verb once, strict refuses the word; words with no fufu verb are never touched.
