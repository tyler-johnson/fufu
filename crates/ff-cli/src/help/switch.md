Move between lines of work: to a branch that is here, to one a remote holds, to a revision, or to new work off trunk. `ff sw` is the short spelling, and `ff start` and `ff new` — jj's name for it — are the same verb: `ff commit` records, `ff switch` continues or begins.

The rule under every spelling is one sentence: find the branch, else mint it. The target is looked up as a branch here (a unique prefix is enough; an ambiguous one lists the candidates), then as a branch a remote holds — bare `spike` or qualified `origin/spike` — then as a revision. A branch here is continued. A remote's branch is minted here under its own name, tracking it, so the next `ff push` sends it where it came from. A revision mints an anonymous branch at that commit. No target at all mints one at trunk's tip, which is what beginning new work means.

-b turns a branch target into a fork — a new branch at its tip, with the branch recorded as its parent — or names the branch a mint would otherwise leave anonymous. Bare -b goes after the target: `ff switch main -b` forks main, where `ff switch -b main` asks for a branch named main. -m describes the change being opened, and is refused on a switch that opens nothing.

Whatever is open here stays with the branch you are leaving as its open commit — the `@` row's sha, at `refs/fufu/open/<branch>`, where `git log --all` shows it — and whatever was parked where you are going comes back exactly as you left it — same files, same edits, same pending description. Both halves are reported, so you always know where your work went and what came back. A minted branch opens clean, with one exception: `@` as the target forks at the commit under the open change and the new branch carries a copy of it — same files, same edits, same description, one sha on both branches — while the branch you left keeps its own, parked.

A parked change comes back over a tip that moved by replaying its one commit there, same change id. When that replay conflicts, the switch still happens and the branch is held: exit 3, `ff status` says so, `ff resolve` lays the change into the working copy with markers, and `ff resolve --abandon` drops it. The index is not carried: a staged hunk comes back as an unstaged edit.

No spelling creates a commit, and every one is one operation: `ff undo` takes back the mint, the copy, and the move together.

## Examples

```
ff switch main
ff switch uni                  a unique prefix is enough
ff switch spike                only origin has it: minted here, tracking it
ff start                       new work, forked from trunk
ff start -m "the next thing"   …with the new change already described
ff start -b hotfix             name the branch at birth
ff switch main -b              fork main onto an anonymous branch
ff start 5b7a90e               fork from a specific commit
ff start @ -b spike            fork under the open change, carrying a copy
ff undo                        changed your mind: the move rolls back,
                               and both branches' open changes with it
```
