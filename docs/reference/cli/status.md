# ff status

Where you are and what is uncommitted: the branch, its upstream, the open change, and the files that differ from the commit underneath it. `ff st` is the short spelling.

The files are a diffstat — counts, not content. [`ff diff`](diff.md) is the same change read down to the line, and it sees the untracked files `git diff` does not.

`--json` also carries the orientation an agent asks for first: the checkout root and which worktree this is, the base the branch sits on and how far above it the branch stands, the remote the branch answers to, and the last operation on this worktree with its session — [`ff op log --json`](op-log.md)'s own row, never the read's own capture.

Status is also where drift is loud. Work done behind fufu's back — a plain `git commit`, a rebase run by a tool that never heard of fufu — is absorbed into the operation log lazily, and status keeps reporting it until the next fufu operation, so foreign motion is never silent. Status reports the motion as one line, the count and the shape of what moved, and [`ff op show @`](op-show.md) lists every ref it moved.

The tracking refs the counts are measured against are kept fresh on a cadence: at most once per `fufu.autoFetch` (ten minutes by default), a fetch rides an ff command before the verb runs, pruning the copies the remote no longer has. `--fetch` runs it now and `--no-fetch` skips it, and [`ff pull`](pull.md) is still the verb that moves your branches.

## Usage

```
Usage: ff status [OPTIONS]

Options:
      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff status
ff status --json               the same state, for scripts
ff diff                        the same change, with content
```
