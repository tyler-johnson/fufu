a friendlier interface to plain git

fufu snapshots your working copy, records commits without staging, and keeps uncommitted work with each branch when you switch. Bare `ff` shows a map of recent branches and parked changes.

## Examples

```sh
ff                              # Show recent branches
ff status                       # Inspect uncommitted work
ff commit -m "parser: fix escapes"
ff switch main                  # Leave this work parked on its branch
ff history                      # See available undo steps
ff undo                         # Undo one local step
ff help commit                  # Read a command's complete help
```

### Options

### Setup and snapshot coverage

Run `ff init` in a new or existing repository, or `ff clone` to copy one. Install shell and agent hooks with `ff hook`, then activate the shell configuration or restart and trust the client as instructed. `ff doctor` checks the setup. Hooks are installed per machine, separately from repository initialization.

Readers normally attempt a snapshot; mutators capture after their initial guards. An active shell alias captures typed Git commands, prompt hooks capture at prompts, and agent hooks capture received events. `ff trigger -m "before experiment"` requests a manual snapshot. Unchanged content creates no new capture.

Recovery requires a successful, retained capture. Ignored untracked files, unsaved editor buffers, and content above `fufu.maxFileSize` (50 MiB by default) are excluded; oversized tracked files can retain older index or base content. Undo follows the current worktree's chain and cannot recover uncaptured bytes or reverse remote updates.

### Shared command conventions

- `ff help <command>` shows complete offline help; `ff <command> -h` shows a short summary and options. For Git passthrough use `ff help git`: `ff git --help` belongs to Git.
- `[alias: …]` names another spelling of the same command. Short forms and supported jj names use the same flags and behavior.
- Commands that read remote copies can fetch on `fufu.autoFetch`'s cadence (ten minutes by default). `--fetch` requests a fetch now on commands that support it; `--no-fetch` skips fetching. Neither flag disables update maintenance. `ff pull` updates local branches.
- `-C <dir>` (`--cwd`) runs the entire command from that directory, including relative path arguments. Use `ff -C ../review status` to inspect another worktree.
- `--json` requests JSON output where supported. Git passthrough, client triggers, update, and watch have their own output protocols.
- `--fields <list>` keeps only the named dotted paths of the JSON data, comma-separated; needs `--json`. A path that matches nothing is refused.

### Extensions and agent instructions

If no built-in command matches, `ff <name>` runs `ff-<name>` from PATH. The child receives `FF_REPO` (worktree path), `FF_CONTRACT` (JSON envelope version), and `FF_SESSION` when set.

Agents should load fufu's skill before editing a managed repository. If it is not already in context, print the recovery, rewriting, and conflict instructions:

```sh
ff hook --skill
```
