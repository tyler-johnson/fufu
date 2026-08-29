# Command table

!!! note "Draft stub"
    Planned content, not yet written. jj's git command table is its most-linked page; this one should earn the same. Every row deserves a footnote when the mapping is inexact — most are.

| you'd type in git | in fufu | the difference |
| --- | --- | --- |
| `git status` | `ff status` | futures included: whether the rebase is clean, what sync would do |
| `git add` + `git commit -m` | `ff commit -m` | no staging; the tree is the change |
| `git checkout -b` | `ff start` | always forks from trunk; name comes later |
| `git switch` + `git stash` dance | `ff switch` | parking is automatic, per branch |
| `git commit --amend` / `fixup!` + autosquash | `ff absorb` | restacks above the target itself |
| `git rebase -i` (reword) | `ff describe <rev> -m` | one verb, automatic restack |
| `git rebase origin/main` + `git pull` | `ff sync` | replays in memory, lands only if clean, undoable |
| `git push` | `ff publish` | leased; the four push shapes distinguished by `--dry-run` |
| `git reflog` + archaeology | `ff history`, `ff undo` | whole-repo, refs and tree together |
| `git log` | `ff log`, bare `ff` | the open change is a row; operation ids attached |
| anything else | `ff git <args>` | snapshot first, then git verbatim |
