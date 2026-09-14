List local branches and remote-only branches. Give a name to create a branch without switching to it; with no revision, the new branch starts at trunk's tip. `ff br` and `ff bookmark` are aliases.

## Examples

```sh
ff branch                       # List branches
ff branch --all                 # Include every remote-only branch
ff branch spike                 # Create at trunk without switching
ff branch spike HEAD~2          # Create at a revision
ff branch spike @               # Copy open work to the new branch
ff branch -d old-experiment      # Delete a local branch
ff branch --prune -n            # Preview pruning; still may fetch
ff describe -b unicode-cleanup  # Rename the current branch
```

### Options

### Listing and creating

The list groups chosen names before automatically generated names. Rows show tips, subjects, parked work, pending descriptions, and remote relationships. Remote-only rows represent tracking refs with no local branch tracking them; `ff switch origin/<branch>` creates and checks out a local tracking branch. `--all` removes the remote-only list limit. `--at` and `--at-op` are declared but currently refused for listing.

An explicit revision creates the branch there. A branch target also records that branch as the new base. `@` means the open change: create below it and park a copy on the new branch, without changing current files. `ff start` creates and switches in one command. `ff describe -b` renames the current branch.

### Deleting and recovery

`-d <name>` deletes a local branch and saves its timeline pointer in trash. Its tip and parked work remain pinned by retained operations; `ff undo` restores the branch and timeline. Deletion does not require a merged check and creates no Git stash.

A plain delete leaves the remote copy, tracking ref, and upstream in place. Add `--shared` to delete them too. A seen/tracking mismatch refuses before local deletion. The remote send then checks the seen value as its lease; a remote race or rejection can leave the local branch deleted while the remote copy survives. Inspect the error and use `ff undo` to recover the local deletion. Undo cannot recreate a successfully deleted remote copy.

### Pruning deleted remote copies

`--prune` fetches, then deletes eligible local branches whose remote copies are gone. All three conditions are required: an upstream is configured, its tracking ref is absent, and fufu recorded a previous seen or pushed copy. A never-published branch does not qualify.

Branches with commits the copy never held are kept and reported, as are the current branch, branches checked out elsewhere, and held branches. Use explicit `-d` when you intend to delete a particular branch. Dependents of a pruned branch are assigned its former base. One undo restores the local prune, timelines, base links, and tracking sections.

`--dry-run` previews local deletion; fetch and maintenance can still run. `--no-fetch` uses existing tracking refs. `fufu.pruneGone` enables the same pruning within `ff pull`.
