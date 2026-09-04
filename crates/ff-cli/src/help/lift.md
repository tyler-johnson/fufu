The other direction of absorb: takes paths out of a commit that has already closed and back into the open change — the revision you name, or the one it sits on when you name none. A lift does not attribute hunks either: whole files are what come back out, and a path filter only chooses which of the commit's files they are.

Everything above the target re-parents in the same operation, so a branch inside that range comes along with it. If the lift takes everything the commit held, the commit is dropped, because fufu writes no empty commit. What moves is the commit's identity and the stack above it; no file is copied or renamed in the re-point.

The branches stacked on this one follow it. Once the lift has landed, every local branch whose base resolves to the rewritten branch is replayed onto its new tip, parent before child, in the same operation, so one `ff undo` takes the cascade back with the lift.

A branch above whose replay conflicts is held on its own, with everything above it left alone, and the lift still lands; `ff status` shows the branch waiting. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named.

## Examples

```
ff lift                        take everything out of the commit under it
ff lift --from HEAD~2          take it out of a commit further back
ff lift src/parser.rs          take only that path back out
```
