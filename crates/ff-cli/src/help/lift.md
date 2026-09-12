Moves content out of a run of commits and into one commit. `ff lift` and `ff absorb` are one move with two sets of defaults: `--from <revset>` names the sources, `--into <rev>` names the target, and each verb's word is its defaults and nothing more. Lift moves from the commit under the open change into the open change — take a closed commit's files back out onto disk — and every other shape is the same move with an end named.

The sources are one contiguous run of commits on the branch's line; `--from HEAD~2..` is the two commits above `HEAD~2`, and both come out. The target is any commit on the line — below the run, above it, or a member of it, which takes both sides — or the open change. A lift does not attribute hunks: whole files are what move, and a path filter only chooses which of the sources' files they are, leaving the rest where it was. A source the move empties is dropped, because fufu writes no empty commit, and the report names it.

Everything between and above replays in the same operation, so a branch inside that range comes along with it. What moves is content and the identity of the commits above; no file is copied or renamed in the re-point. A replay that conflicts holds with nothing written, and `ff resolve` opens it.

`-m` gives the target a message: the pending description for the open change, a reword for a closed commit. Without it the target keeps what it had.

### Branches stacked above

The branches stacked on this one follow it. Once the move has landed, every local branch whose base resolves to the rewritten branch is replayed onto its new tip, parent before child, in the same operation, so one `ff undo` takes the cascade back with the move.

A branch above whose replay conflicts is held on its own, with everything above it left alone, and the move still lands; `ff status` shows the branch waiting. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named.

### Hooks

A lift into the open change makes no worktree content into commit content, so no `pre-commit` runs; naming the open change among the sources with `--from` does, and then it runs as it would for a close. `-m` on a closed target runs `commit-msg` the way a reword does. `--no-verify` skips both.

## Examples

```
ff lift                          take everything out of the commit under it
ff lift --from HEAD~2            take it out of a commit further back
ff lift --from HEAD~2..          uncommit the two commits above HEAD~2
ff lift --from HEAD~3 --into HEAD  move a commit's content up into the tip
ff lift src/parser.rs            take only that path back out
```
