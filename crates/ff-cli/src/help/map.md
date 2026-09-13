Show how local branches relate, including their uncommitted parked changes. Bare `ff` runs this same command. By default the map shows recent branches, newest tip first, subject to the configured branch limit.

## Examples

```sh
ff map                          # Recent branches and parked work
ff -n 3                         # The three newest branch tips
ff map --all                    # Every local branch
ff log                          # Individual commits on this branch
```

### Options

### Reading the map

The map shows branch tips, forks, and merges that connect the displayed branches. Runs between them collapse into a `~ N commits` row. History that connects none of the displayed branches, such as a merged and deleted branch, has no row.

Parked changes are uncommitted work saved with a branch when you switch away. `ff switch <branch>` resumes that work. Internal open commit objects are visible to `git log --all`; they are not commits recorded in branch history.

`-n 0` or `--all` removes the local branch limit. The CLI attempts a snapshot and can auto-fetch and run maintenance. `--no-fetch` uses existing remote-tracking refs. Past-state flags are not supported.
