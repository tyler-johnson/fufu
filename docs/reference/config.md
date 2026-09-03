# Configuration

fufu's settings are plain git config under `fufu.<key>`, read and written with [`ff config`](cli/config.md). No subcommands — arity decides: bare `ff config` lists every setting with its value, its meaning, and a `(default)` marker; a key alone prints the effective value; a key plus a value sets it; `--unset` returns it to the default. Keys are case-insensitive and the `fufu.` prefix is optional, so `ff config keep`, `ff config Keep`, and `ff config fufu.keep` all name the same setting.

## Where values live

A plain `ff config <key> <value>` writes this repo's config; `--global` writes user-level git config instead, so the value applies to every repo. Precedence between the scopes is git's own — environment overrides, then repo, then global, then system — and when a non-default value applies, `ff config` says which scope it came from.

Because storage is ordinary git config, `git config fufu.keep` reads and writes the very same value, and the two tools can never disagree. What `ff config` adds over raw `git config` is the registry below: it knows what settings exist and what they default to, and it validates a new value through the same parser that will later read it, so a typo is refused before it touches disk. Set the same typo with raw `git config` and every fufu reader quietly falls back to its default — the setting looks set and does nothing.

## Policy keys are written from a shell

`fufu.gitPolicy` and `fufu.toolPolicy` decide what fufu refuses an agent, so an agent that could set its own tier through the tool policing it is not policed at all. A write to either through the [`ff mcp`](cli/mcp.md) tool is refused with `usage/mcp-policy-write`, naming the shell as the place to make it; `--unset` counts as a write, since taking a value away lowers the tier to the default. The same command typed at a shell writes, which is where a person changing their own policy already is.

Reading is untouched: through the tool, bare `ff config` still lists every setting and a key alone still prints what applies. Every other setting writes through the tool as before.

## The pager

[`ff log`](../reference/cli/log.md), [`ff evolog`](../reference/cli/evolog.md), and [`ff op log`](../reference/cli/op-log.md) page their output, but only when stdout is a real terminal and the view is human — pipes, scripts, and `--json` always get plain direct bytes. Which pager runs, in precedence order:

1. `fufu.pager` git config — when set, it overrides both environment variables.
2. `FF_PAGER`.
3. `PAGER`.
4. `less`.

The value is whitespace-split with no shell quoting, and `cat` means no pager — git's own convention. When the pager runs and `LESS` or `LESSCHARSET` is unset, fufu supplies `LESS=FR` (quit if one screen, keep ANSI colors) and `LESSCHARSET=utf-8`. A pager that fails to spawn falls back to direct printing, silently.

## Settings

Every setting `ff config` knows, rendered from the registry in `crates/ff-cli/src/cmd/config.rs` — the same source bare `ff config` lists.

<!-- registry:begin — generated from registry() in crates/ff-cli/src/cmd/config.rs by a test; edit there, then make docs-gen -->

### maxFileSize

`fufu.maxFileSize` — size; default `52428800`

Largest new file a snapshot will include, in bytes (52428800 = 50 MiB). Suffixes work: 100M, 1G. Bigger files are skipped and the snapshot message lists them.

### keep

`fufu.keep` — duration; default `90d`

How long operations live: ff trim drops everything past the cutoff, captures and verbs alike. Compact durations (30d, 36h, 2w, 45s); a bare number means days.

### autoTrim

`fufu.autoTrim` — cadence; default `1d`

How often retention enforces itself: a trim rides an ff command at most this often, per repo. false leaves trimming entirely to `ff trim`; durations work too (12h, 2w), floored at one minute.

### pager

`fufu.pager` — command; default `less`

Pager for ff log and ff evolog on a TTY. When set it overrides FF_PAGER and PAGER; whitespace-split, no shell quoting; cat means no pager.

### updateCheck

`fufu.updateCheck` — cadence; default `1d`

How often ff looks for a new release in the background. false turns the whole machinery off (checks and notices); true means daily; durations work too (12h, 7d, 2w), floored at one minute.

### trunk

`fufu.trunk` — branch; unset by default

Which branch is trunk: what ff sync rebases onto, what ff status measures against, and where a bare ff start forks from. Local (main) or remote-qualified (origin/main). Unset means fufu works it out.

### theme

`fufu.theme` — choice of `muted`, `vivid`, `terminal`; default `muted`

Color theme for ff output. muted gives desaturated 256-color (the default); vivid the saturated cut; terminal the base sixteen so your own terminal theme decides the actual hues.

### gitPolicy

`fufu.gitPolicy` — choice of `observe`, `coach`, `strict`; default `coach`

What fufu says when git is reached for directly — through ff git, or in an agent's own shell. observe records and stays quiet; coach (the default) names the fufu verb once per word; strict refuses the words fufu has verbs for. Nothing is ever silently run in its place.

### toolPolicy

`fufu.toolPolicy` — choice of `observe`, `coach`, `strict`; default `strict`

What fufu says when an agent runs ff in its shell while the ff tool is up for it. observe stays quiet; coach names the tool once per session; strict (the default) refuses and names the call to make instead. What it speaks to is what the tool serves: a builtin verb or a declared extension. The shell-only verbs pass, and so does an ff <name> the tool will not serve, since a shell is the only place that one runs. Nothing is said at all when no fufu server is serving that client.

### futuresDepth

`fufu.futuresDepth` — size; default `200`

How many commits ff will replay when simulating a rebase. Past this many, the verdict is an honest "can't simulate" rather than a slow one. Suffixes work: 1k.

### watchInterval

`fufu.watchInterval` — size; default `200`

How often ff watch re-reads the operation log's tip, in milliseconds. A tick reads two refs and nothing else, so the default costs well under a millisecond five times a second. Suffixes work: 1k.

### mapDepth

`fufu.mapDepth` — size; default `1000`

How many commits bare ff walks before it stops and says so with a trailing ~. The map is a skeleton of branch tips and forks, so this caps the walk, not the rows. Suffixes work: 2k.

<!-- registry:end -->

## What fufu reads from git's config

The `fufu.*` keys above are fufu's own. Everything else fufu needs, it reads from git's existing configuration rather than keeping a second copy:

- **Identity.** Commits and operations are authored from `user.name` and `user.email`. With neither set, fufu refuses with the same fix git would ask for: `git config user.name <name>`, `git config user.email <email>`.
- **URLs, proxies, and credentials.** [`ff sync`](../reference/cli/sync.md) and [`ff clone`](../reference/cli/clone.md) speak the git protocol natively, but they honor `url.<base>.insteadOf` rewrites, `http.proxy`, and `credential.helper` from your git config, and they invoke your credential helpers and `ssh` exactly as git would. Push runs the git binary itself, so everything that configures a git push applies unchanged.

The practical consequence: a repo that already fetches through a corporate proxy or authenticates through a credential helper keeps working under fufu with nothing new to configure. fufu adds settings only for behavior git does not have.
