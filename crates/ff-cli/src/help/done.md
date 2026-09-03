Ends the editing session `ff edit` opened: the commit the session was opened on is amended with what the working tree now holds, what waited ahead is replayed onto it, and you land back on the branch the session left standing.

A replay that would conflict stops with nothing changed rather than leaving you mid-rewrite. `--abandon` drops the session instead of landing it, stashing whatever is uncommitted rather than discarding it.

It is one operation — the amend, the replay and the return move together — so one `ff undo` takes the whole session back.

The branches stacked on the branch you land on follow it. Once the session has landed, every local branch whose base resolves to that branch is replayed onto its new tip, parent before child, in the same operation, so one `ff undo` takes the cascade back with the session. A branch above whose replay conflicts is held on its own, with everything above it left alone, and the session still lands; `ff status` shows the branch waiting. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named. Landing a resolution does the same from the branch the hold stood on, which is how the branches a hold stopped resume once it lands.

The session's content is about to become the amended commit's content, so your `pre-commit` hook runs over it, and a hook that exits non-zero refuses the landing with the session still open. A session that also carries a new description runs the message hooks over that description. Landing a resolution — the `ff done` that finishes `ff resolve` — runs `pre-commit` too, the way `git rebase --continue` does. `--no-verify` skips them; `--abandon` runs none, since nothing is being committed.

## Examples

```
ff done                        amend, replay what waited, land back
ff done --abandon              drop the session, stash what is open
ff done --no-verify            land without running the hooks
```
