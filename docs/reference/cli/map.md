# ff map

Show how local branches relate, including their uncommitted parked changes. Bare `ff` runs this same command. By default the map shows recent branches, newest tip first, subject to the configured branch limit.

## Usage

```
Usage: ff map [OPTIONS]
```

## Examples

```sh
ff map                          # Recent branches and parked work
ff -n 3                         # The three newest branch tips
ff map --all                    # Every local branch
ff log                          # Individual commits on this branch
```

## Options

```
Options:
  -n, --max-count <count>
          Branches to show, newest tip first; 0 means all

      --all
          Every local branch

      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

      --fields <list>
          Keep only these dotted paths of the JSON data, comma-separated

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Reading the map

The map shows branch tips, forks, and merges that connect the displayed branches. Runs between them collapse into a `~ N commits` row. History that connects none of the displayed branches, such as a merged and deleted branch, has no row.

Parked changes are uncommitted work saved with a branch when you switch away. [`ff switch <branch>`](switch.md) resumes that work. Internal open commit objects are visible to `git log --all`; they are not commits recorded in branch history.

`-n 0` or `--all` removes the local branch limit. The CLI attempts a snapshot and can auto-fetch and run maintenance. `--no-fetch` uses existing remote-tracking refs. Past-state flags are not supported.
