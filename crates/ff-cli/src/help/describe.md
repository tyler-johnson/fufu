The open change carries a description before it enters branch history, so you can name work while you are doing it and let `ff commit` pick the name up when it closes. Describing rewrites the internal open commit object without moving the branch tip. -m sets it inline; the bare form opens $EDITOR seeded with the current text. `ff desc` is the short spelling, and jj's too.

-b names the branch you are on instead — the same act whether it is an anonymous petname earning a real name or a chosen name being replaced. The capture chain, the open commit, and the pending description all come along, which is the part a bare `git branch -m` would orphan.

Naming a revision rewords a commit that has already closed instead. Everything above it re-parents in the same operation, so any branches sitting inside that range come along with it.

### Hooks

A reword authors a message for a commit, so your `prepare-commit-msg` and `commit-msg` hooks run over it, and a hook that exits non-zero refuses the reword before anything is planned; `--no-verify` skips `commit-msg`. No tree moves, so `pre-commit` does not run. The bare form updates the internal open commit's pending description and runs no hook at all — they fire when the change closes.

### Branches stacked above

The branches stacked on this one follow a reword. Once the reword has landed, every local branch whose base resolves to the reworded branch is replayed onto its new tip, parent before child, in the same operation, so one `ff undo` takes the cascade back with the reword.

A reword preserves its commit's tree, but a stacked branch that was already out of date can still conflict during the cascade. That branch records a hold while the reword stands. This outcome currently exits 0; scripts must inspect `reword.cascade.held`. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named, with everything above it left alone.

## Examples

```
ff describe -m "parser: handle unicode escapes"
ff describe                    open $EDITOR on the pending description
ff describe -b unicode-cleanup name the branch you are on
ff describe HEAD~2 -m "fix"    reword a closed commit, restacking above it
```
