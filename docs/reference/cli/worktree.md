# ff worktree

List this repository's worktrees and retained chains from removed worktrees. Give a path to create another checkout, or `-d` to remove one after capturing its work. Each worktree has its own operation chain and undo history. `ff workspace` is an alias.

## Usage

```
Usage: ff worktree [OPTIONS] [path] [branch]
```

## Examples

```sh
ff worktree                     # Live checkouts and removed chains
ff worktree ../review           # Create a checkout and branch
ff -C ../review status          # Inspect that checkout explicitly
ff worktree -d ../review        # Capture its work, then remove it
```

## Options

```
Arguments:
  [path]
          Create a checkout at this path with its own operation chain

  [branch]
          Branch to check out; omitted creates one named after the directory

Options:
  -d, --delete <worktree>
          Capture eligible work and remove a worktree, by path or listed ID

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

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

## Creating and using a worktree

`ff worktree <path> [<branch>]` creates a checkout sharing this repository's objects and refs, with its own files, index, HEAD, and lock. With no branch, it creates one named after the directory, or an automatic name if that is taken. A branch checked out elsewhere is refused.

Creation records an earliest recovery point. A worktree created outside fufu gets that point on its first fufu command; earlier uncaptured state is not available to undo. Use `ff -C <path>` for commands in another worktree, or explicitly change directory there.

## Removal and file recovery

`-d` accepts a path or worktree ID from the list. It captures eligible content into that worktree's chain before removing the checkout, and prints the capture ID. There is no `--force` requirement for dirty work because the captured content remains in history.

Use the exact printed ID with [`ff restore <path> --at-op <id>`](restore.md) in a surviving checkout to recover files. Removed chains remain visible in the list, but `fufu.keep` retention still applies. Ignored untracked files, unsaved buffers, and content above `fufu.maxFileSize` are not preserved by that capture; oversized tracked files can retain older content.

## Listing details

Live worktrees appear first with paths and branches, followed by removed worktree chains and their operation tips. Listing declares `--at` and `--at-op` but currently refuses them. Undo follows the chain of the worktree in which it runs, not every checkout's history at once.
