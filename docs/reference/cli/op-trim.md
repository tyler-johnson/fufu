# ff op trim

Remove operations older than `fufu.keep` (90 days by default). Most users can rely on automatic trimming, which runs at most once per `fufu.autoTrim` interval per worktree, daily by default. `ff trim` is the older, unlisted spelling.

## Usage

```
Usage: ff op trim [OPTIONS]
```

## Examples

```sh
ff op trim -n                   # Preview operations to remove
ff op trim                      # Apply retention
ff op trim --gone               # Also drop pointers for deleted branches
ff config keep 30d              # Set a shorter retention window
ff config autoTrim false        # Trim only when requested manually
```

## Options

```
Options:
  -n, --dry-run
          Preview retention without dropping operations; capture can still run

      --gone
          Also drop the pointers of branches that no longer exist

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

## Side effects and recovery

`--dry-run` previews without dropping operations. The CLI still attempts capture and can run update maintenance. A real manual trim also invokes `git gc --auto --quiet`, even when no operations are dropped. Automatic trimming skips that invocation.

Before trimming, the old chain tip is saved under `refs/fufu/wt/<worktree>/trash/@ops`, making the last trim recoverable. Retention still limits long-term recovery; discarded content is not promised indefinitely.

## Retained records

Surviving operations keep their trees, messages, and dates. Their predecessor links are rewritten, which can change operation IDs. The reflog preserves original times so time-based lookups still use the recorded dates.
