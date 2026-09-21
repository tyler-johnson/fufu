# Configuration

Use [`ff config`](cli/config.md) to read or change settings stored in Git configuration under `fufu.<key>`. Run it inside a repository, including when using `--global`.

```sh
ff config                       # List settings and descriptions
ff config keep                  # Read the effective retention value
ff config keep 30d              # Set this repository's retention
ff config --global pager cat    # Disable paging by default in your repositories
ff config --unset keep          # Remove this repository's override
```

Keys are case-insensitive and the `fufu.` prefix is optional, so `ff config keep`, `ff config Keep`, and `ff config fufu.keep` all name the same setting.

## Where values live

A plain `ff config <key> <value>` writes the repository's shared `.git/config`, including when run from a linked worktree. `--global` writes user-level Git configuration. Effective values follow Git precedence: environment overrides, repository configuration, global configuration, then system configuration. A built-in default applies when no configured value exists. `ff config --json` includes each value's source; the human list marks built-in defaults with `(default)`.

For example, in a repository with no other `keep` overrides:

```sh
ff config --global keep 60d
ff config keep 30d
ff config keep                  # 30d from this repository
ff config --unset keep
ff config keep                  # 60d from global configuration
ff config --global --unset keep
ff config keep                  # 90d built-in default
```

`--unset` removes a value in the selected scope; it does not force the built-in default. A higher-precedence environment override also continues to apply. A one-command override in a POSIX shell looks like this:

```sh
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=fufu.keep GIT_CONFIG_VALUE_0=7d ff config keep
```

`git config fufu.keep` reads the same stored key, but does not supply fufu's built-in default. `ff config` validates writes. Raw Git configuration can contain an invalid value: some readers fall back to a default, while invalid `keep` makes trimming fail. [`ff doctor`](cli/doctor.md) reports invalid settings; correct or unset them in the scope that supplies them.

## Common settings

- **Recovery:** `keep` sets the age window (90 days by default); `autoTrim` sets when eligible commands check retention (daily by default, with a separate timer per worktree). Trimming processes the current worktree's log and orphan logs left by removed worktrees; other live worktrees keep their own schedules. Old captures and recorded operations age out together, and a log with no surviving operations can be removed. Preview with [`ff op trim --dry-run`](cli/op-trim.md). See [retention and the earliest recovery point](../guides/recovery.md#retention-and-the-earliest-recovery-point).
- **Snapshot size:** `maxFileSize` defaults to 50 MiB, in bytes. Oversized untracked files and oversized modified tracked content are skipped; index or base content may remain. Ignored untracked files and unsaved editor buffers are also outside [snapshot coverage](../concepts/snapshots-and-undo.md#coverage-and-limits), regardless of this limit.
- **Network:** `autoFetch false` disables automatic fetches; explicit `--fetch` and [`ff pull`](cli/pull.md) still fetch. `--no-fetch` skips fetching for one supported invocation. Native HTTP proxy limits are [below](#what-fufu-reads-from-gits-config).
- **Git commands:** `gitPolicy` defaults to `coach`. Read [policy behavior and client limits](../agents/setup.md#pick-a-git-policy) before choosing `strict`.
- **Conflicts:** `onConflict` defaults to `hold`: a conflicting [`ff restack`](cli/restack.md), [`ff pull`](cli/pull.md), or [`ff merge`](cli/merge.md) records a hold and stops. `ff config onConflict resolve` also opens the current branch's resolution session. Use `--resolve` or `--no-resolve` to override the setting for one run.
- **Pulling:** `pull` defaults to `auto`: `ff pull`'s base step replays a straight line onto a moved base and leaves a branch standing once its commits hold any merge, reporting it behind. `replay` replays every branch, flattening a merge of the base; `merge` leaves every branch behind its base standing. `fufu.<pattern>.pull` chooses per branch by refspec glob, `*` crossing `/`, and the last matching row in git's read order wins, so a repository row beats a global one. Written as a git config subsection, since `ff config` reads these rows and does not write them:

  ```ini
  [fufu "tyler/*"]
      pull = replay
  ```

  `ff config pull` lists the setting and every pattern row with its scope.
- **Display:** `pager cat` disables paging; `theme terminal` uses your terminal's base colors.

Duration values use `s`, `m`, `h`, `d`, or `w`; bare numbers mean days. Cadences (`autoTrim`, `autoFetch`, `updateCheck`) also accept `true` for that setting's default and `false` to disable it, and clamp explicit durations to at least one minute. `0` disables a cadence; `0d` is a duration and becomes one minute. Git size suffixes use powers of 1024: `1k` is 1024, `1M` is 1048576.

## Settings

The full list of supported settings, defaults, and accepted values follows.

<!-- registry:begin — generated from registry() in crates/ff-cli/src/cmd/config.rs by a test; edit there, then make docs-gen -->

### maxFileSize

`fufu.maxFileSize` — size; default `52428800`

Maximum regular-file size in bytes for working-copy snapshots (50 MiB). Oversized untracked and modified tracked content is skipped; index or base content can remain. Git size suffixes work: 100M, 1G.

### keep

`fufu.keep` — duration; default `90d`

Retention window for captures and recorded operations, applied by ff op trim and automatic trimming to the current and removed-worktree logs. Default: 90 days. Units: s, m, h, d, w; bare numbers mean days.

### autoTrim

`fufu.autoTrim` — cadence; default `1d`

Automatic trim cadence, checked after eligible commands, per worktree. true means daily; false disables automatic trimming. Durations (12h, 2w) have a one-minute minimum; bare numbers mean days. Skipped in CI.

### pruneGone

`fufu.pruneGone` — bool; default `false`

Let ff pull prune local branches whose remote copy is gone, using the ff branch --prune guards. Branches with unpublished commits are kept. Disabled by default.

### autoFetch

`fufu.autoFetch` — cadence; default `10m`

Automatic fetch cadence per repository: true means 10 minutes; false leaves fetching to ff pull and --fetch. Some commands fetch every run when enabled. Durations have a one-minute minimum; bare numbers mean days.

### pager

`fufu.pager` — command; default `less`

Pager for ff log, ff evolog, and ff op log on a TTY. Overrides FF_PAGER and PAGER; whitespace-split, no shell quoting; cat means no pager.

### updateCheck

`fufu.updateCheck` — cadence; default `1d`

Background release-check cadence. true means daily; false disables checks and notices. Durations (12h, 7d, 2w) have a one-minute minimum; bare numbers mean days.

### trunk

`fufu.trunk` — branch; unset by default

Trunk branch used for default branch creation and as a fallback base for status and pull. Accepts local (main) or remote-qualified (origin/main) names. Unset means automatic detection.

### theme

`fufu.theme` — choice of `muted`, `vivid`, `terminal`; default `muted`

Output colors: muted uses desaturated 256-color shades; vivid uses saturated shades; terminal uses your terminal's base sixteen colors.

### gitPolicy

`fufu.gitPolicy` — choice of `observe`, `coach`, `strict`; default `coach`

Policy for covered Git writes through ff git and Claude Code hooks: observe stays quiet; coach suggests a fufu command; strict refuses. Codex, Qwen Code, OpenCode, Copilot CLI, and Cursor hooks tally writes but send no policy reply. Commands are never silently translated.

### onConflict

`fufu.onConflict` — choice of `hold`, `resolve`; default `hold`

When ff restack, ff pull, or ff merge conflicts on the current branch, hold records the hold and stops; resolve also opens the resolution session. Both outcomes exit 3. --resolve and --no-resolve override it for one run.

### pull

`fufu.pull` — choice of `auto`, `replay`, `merge`; default `auto`

How ff pull's base step takes a moved base in: replay replays the branch's commits onto it; merge leaves the branch standing and reports it behind; auto replays a straight line and leaves a branch standing once its commits hold any merge. fufu.<pattern>.pull overrides it for matching branches (refspec globs, * crosses /; the last match in git's read order wins, so a repository value beats a global one).

### futuresDepth

`fufu.futuresDepth` — size; default `200`

Maximum commits replayed in a rebase simulation. Larger simulations report that they cannot be simulated. Git size suffixes work: 1k.

### watchInterval

`fufu.watchInterval` — size; default `200`

Polling interval for ff watch, in milliseconds (default: 200). Git size suffixes work: 1k means 1024 milliseconds.

### mapDepth

`fufu.mapDepth` — size; default `1000`

Maximum commits walked by the branch map. A trailing ~ marks a truncated walk. This limits commits visited, not displayed rows. Git size suffixes work: 2k.

<!-- registry:end -->

## The pager

[`ff log`](cli/log.md), [`ff evolog`](cli/evolog.md), and [`ff op log`](cli/op-log.md) use a pager only for human output on a terminal. Pipes and `--json` receive output directly. Pager selection is:

1. `fufu.pager`, when configured.
2. `FF_PAGER`.
3. `PAGER`.
4. `less`.

The value is split on whitespace without shell quoting; `cat` disables paging. When unset, fufu supplies `LESS=FR` (quit if one screen, preserve ANSI colors) and `LESSCHARSET=utf-8`. If the pager cannot start, fufu prints directly.

## What fufu reads from git's config

Fufu also reads existing Git settings:

- **Identity:** `user.name` and `user.email` supply your commit identity. Fufu resolves the Git committer identity for both author and committer when creating a new change, so `GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` override it; `GIT_AUTHOR_*` does not independently select a new author. Replays preserve the original author. Operation-journal commit objects use `fufu <fufu@local>`. [Signing](signing.md) uses Git's signing configuration.
- **URLs and credentials:** `ff pull` and [`ff clone`](cli/clone.md) use native transport, read `url.<base>.insteadOf` and `credential.helper`, invoke credential helpers, and use `ssh` for SSH URLs. The native HTTP backend does not honor `http.proxy`. [`ff push`](cli/push.md) uses the Git binary and its transport configuration.

For an HTTP proxy, use [`ff git fetch`](../reference/cli/git.md) followed by `ff pull --no-fetch`, or `ff git clone` followed by [`ff init`](../reference/cli/init.md). Disable `fufu.autoFetch` when native automatic fetches cannot reach the remote.
