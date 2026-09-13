# ff restore

Discard edits to selected files by restoring their content from the commit below the open change. Paths are required unless you use `--all`, which restores the entire working copy and removes files absent from the source.

## Usage

```
Usage: ff restore [OPTIONS] [path]...
```

## Examples

```sh
ff restore src/main.rs          # Discard this file's edits
ff restore --all                # Discard all eligible working-copy edits
ff restore --all --at 2h        # Recover files from two hours ago
ff restore docs/ --at-op '@^'   # Recover a directory from an operation
ff restore src/ --from main~2   # Recover files from a commit
```

## Options

```
Arguments:
  [path]...
          Paths to restore from the source

Options:
      --from <rev>
          Source revision; defaults to the commit below the open change

      --all
          Restore the entire worktree to the source state

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

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Alternate sources

```text
--from <rev>      One recorded revision: branch, SHA, or change ID
--at-op <op>      One operation: hexadecimal ID, prefix, or @ suffix
--at <time>       Operation current at a time: 30m, 2h, 3d, or a date
```

Choose one source flag. `--from` must resolve to exactly one commit and refuses the open change `@`. In operation space, `@` is the live tip, `@^` its predecessor, and `@~3` three predecessors back; sets and functions are not accepted here. The source resolves before the mandatory pre-restore snapshot. See [paths and sources](../revisions.md#paths-sources-and-past-state-reads).

Paths are files or directory prefixes, without globs. Recovery can only use retained captured content: ignored untracked files, unsaved buffers, and oversized content are not recoverable from snapshots that excluded them.

## Effects and recovery

Restore writes worktree files only. It leaves the index, HEAD, and branch refs in place. If its pre-restore capture fails, it writes no files. [`ff undo`](undo.md) takes the restore back; another restore can also recover from that pre-operation snapshot. Use [`ff op restore`](op-restore.md) to restore branch state and the index together with files.
