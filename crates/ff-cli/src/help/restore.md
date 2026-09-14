Discard edits to selected files by restoring their content from the commit below the open change. Paths are required unless you use `--all`, which restores the entire working copy and removes files absent from the source.

## Examples

```sh
ff restore src/main.rs          # Discard this file's edits
ff restore --all                # Discard all eligible working-copy edits
ff restore --all --at 2h        # Recover files from two hours ago
ff restore docs/ --at-op '@^'   # Recover a directory from an operation
ff restore src/ --from main~2   # Recover files from a commit
```

### Options

### Alternate sources

```text
--from <rev>      One recorded revision: branch, SHA, or change ID
--at-op <op>      One operation: hexadecimal ID, prefix, or @ suffix
--at <time>       Operation current at a time: 30m, 2h, 3d, or a date
```

Choose one source flag. `--from` must resolve to exactly one commit and refuses the open change `@`. In operation space, `@` is the live tip, `@^` its predecessor, and `@~3` three predecessors back; sets and functions are not accepted here. The source resolves before the mandatory pre-restore snapshot. See [paths and sources](../revisions.md#paths-sources-and-past-state-reads).

Paths are files or directory prefixes, without globs. Recovery can only use retained captured content: ignored untracked files, unsaved buffers, and oversized content are not recoverable from snapshots that excluded them.

### Effects and recovery

Restore writes worktree files only. It leaves the index, HEAD, and branch refs in place. If its pre-restore capture fails, it writes no files. For precise recovery, find the pre-restore snapshot with `ff op log` and restore from its ID. `ff undo` groups consecutive captures and can step past the individual file state you want. Use `ff op restore` to restore branch state and the index together with files.
