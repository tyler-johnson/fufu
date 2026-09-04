a friendlier interface to plain git

fufu snapshots your working tree as you work — before every command it runs, before every git command you type through the alias, before every tool call your agent makes — so the last hour of work is always recoverable. You never type a capture: every verb takes one first.

Snapshots are ordinary git objects under refs/fufu/, beside your history rather than in it: nothing fufu stores reaches a remote, and nothing it stores needs fufu to read back.

### What the map draws

Bare `ff` is the map: recent work across every branch, parked changes included — where you left things. It draws the commits that relate the branches shown — their tips, the forks where they part, the merges that land one — and contracts the runs between them into one `~ N commits` row. History that relates only itself, like a merged-and-deleted branch, earns no row.

### Spelling a command

- Seven verbs take a short spelling: st, ci, sw, br, ev, desc, cfg — for status, commit, switch, branch, evolog, describe, and config.
- Every verb takes `-C <dir>` (`--cwd`) as well: run as if fufu had been started in `<dir>`, git's spelling of the same idea. It is a chdir, so a relative path argument after it reads from `<dir>` too, and any directory inside the repository you mean will do — a linked worktree included, which is how you ask one worktree a question without leaving another.
- `ff <name>` runs `ff-<name>` from PATH when no verb matches, git-style. The child inherits three variables: FF_REPO, the worktree it was invoked against; FF_CONTRACT, the version number every --json envelope carries; and FF_SESSION, the session tag when one is set.

## Examples

```
ff                             the map: where you left things
ff -n 3                        just the three branches you touched last
ff --all                       every local branch, however old
ff log                         the timeline: commits wearing their operations
ff restore src/ --at 2h        a directory, as it was two hours ago
ff undo                        roll the whole repo back one run of work
ff -C ../bay status            another worktree, without leaving this one
```

Wire it in, so capture reaches the commands you did not type:

```
ff hook                        what is on this machine, then asks
ff hook claude zsh             wire exactly those
ff doctor                      is any of this actually on?
```

`ff help <command>` (or `ff <command> --help`) has the details.

Are you an agent working in a repository fufu manages? Before you write to it:

```
SKIP if fufu's skill is already in your context. Otherwise:
ff hook --skill                the manual: recovery, rewriting, conflicts
```
