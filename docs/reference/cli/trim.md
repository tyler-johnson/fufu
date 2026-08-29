# ff trim

Retention with an undo. The log's pre-trim tip is written to the chain's own trash ref, `refs/fufu/wt/<worktree>/trash/@ops`, before a single ref moves, so the last trim is itself recoverable. Survivors keep their trees, messages, and dates byte-for-byte — only parent slots relink — and the reflog is replayed with the original times, so `--at 2h` stays truthful afterwards.

You rarely need to run this. A trim rides an ff command at most once per fufu.autoTrim (daily by default), per worktree. This is the hand-run form, and the only one that nudges git's own gc when it dropped something.

## Usage

```
Usage: ff trim [OPTIONS]

Options:
  -n, --dry-run
          Report what would be dropped without writing anything

      --gone
          Also drop the pointers of branches that no longer exist

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff trim -n                     preview: what would go, nothing written
ff trim                        drop everything past the keep window
ff trim --gone                 also drop pointers whose branch is gone
ff config keep 30d             a shorter window
ff config autoTrim false       leave trimming entirely to this command
```
