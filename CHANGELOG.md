# Changelog

## Unreleased

### Added

- `prepare-commit-msg` and `post-commit`, the two commit-time hooks fufu did not implement. `ff commit` now runs all four: `pre-commit`, `prepare-commit-msg`, `commit-msg`, `post-commit`. Every commit hook runs with `GIT_EDITOR=:`, as git sets it for a command that will not open an editor.
- `--no-verify` on `ff absorb`, `ff done`, and `ff describe <rev>`, the verbs that can now be declined by a hook.
- `ff hook powershell`. `$PROFILE` gets `function git { ff git @args }` and a wrapped `prompt`, with the same marker and the same `ff unhook` as the other shells. On Windows the profile is PowerShell 7's under the Documents known folder, or Windows PowerShell 5.1's when that is the only one on disk, and the slug is always detected; elsewhere it is `~/.config/powershell/`, detected when the profile exists or `$SHELL` is `pwsh`.

### Changed

- `ref/contended` exits 4 instead of 1, the one code that means nothing was touched and the same command run again is the answer. 3 keeps meaning a human is needed.
- `ff update` names the command that updates this copy of fufu instead of downloading a binary over itself: `cargo install` for a source build, `brew upgrade fufu` for Homebrew, the install script for a binary at the install script's own path, and the releases page for anything else. It runs that command only on `-y` or a typed yes, and `-y` on a channel it cannot drive exits 1. The only binary fufu will ever replace is the one at the install script's own path, and the install script is what replaces it.
- The background update check still runs on `fufu.updateCheck` and still lands a one-line notice, but nothing installs itself any more. The notice names whichever command owns the binary.
- `install.sh` and `install.ps1` land the new binary beside the old one and rename, rather than writing over it, so they can replace an `ff` that is currently running.

### Removed

- `fufu.autoUpdate`. Silent background installs are gone; there is nothing left for the setting to turn off.
- The in-process downloader — asset selection, sha256 verification, archive extraction, and the binary swap — along with the four dependencies it needed: `sha2`, `tar`, `flate2`, and the windows-only `zip`.

### Fixed

- A CRLF rc file keeps its line endings through `ff unhook` and through the retired-spelling rewrite `ff hook` does; both rejoined the file with LF.
- `ff absorb`, `ff done`, and `ff describe <rev>` ran no hooks at all, so content a `pre-commit` gate would decline on a close landed in the commit anyway. `pre-commit` now runs for `ff absorb` and for both of `ff done`'s landings — the edit session and the resolution — over the index staged with exactly what is landing, and the message hooks run for `ff describe <rev>` and for an `ff done` whose session carries a new description. `ff lift`, `ff restack` and `ff sync` still run none, matching `git rebase`.
- `ff sync` failed its whole run when a linked worktree's admin dir under `.git/worktrees/` held a `gitdir` file without a readable `commondir` — the state such a directory passes through while it is being created or removed. git ignores such a directory; fufu's native fetch stopped on it. The fetch is now retried once through `git fetch`, which walks past it, and the error names the offending admin dir when that fails too.
- A held rewrite whose later commit merged its own change *into* a standing conflict marker left `ff done` unable to land any resolution: the block `ff resolve` showed was no longer the block the step that owned it had written, and every fix was refused with `no marker block to resolve at <path>`. The chain now stops at that fold, the same way it stops when two conflicts interleave, so `ff resolve` shows the mark as its owning commit wrote it and `ff done` lands the fix.

## v0.10.0 — 2026-09-01

### Added

- Documentation site at <https://tyler-johnson.github.io/fufu/>: tutorial, concepts, task guides, CLI reference, and a section on running fufu behind an agent.
- Commit signing, using git's own configuration (`commit.gpgsign`, `gpg.format`, `user.signingkey`, and the program keys) in all three formats git supports: openpgp, x509, and ssh. Rewrites sign as well. `ff commit` accepts `-S` and `--no-sign`.
- `ff log` and `ff status` mark signed commits; `ff log --signatures` verifies them and reports the verdict, tool, and key; `ff show` verifies the commit it prints; `ff doctor` reports whether the signing setup will work.
- `fufu.gitPolicy`, with levels `observe`, `coach` (default), and `strict`, covering both `ff git` and a bare `git` run inside an agent's shell tool. `ff doctor` reports the tally.

### Changed

- `ff --help` groups its commands under the same headings `git help` uses, instead of one alphabetical list. `ff -h` shows a short list of common verbs.
- The CLI reference and the config key list are generated from the binary's help pages and checked byte for byte in CI.
- Subagents, and repositories an agent has just entered, now receive the agent briefing. Claude Code's plugin installs `Stop` and `SubagentStop` capture events.

### Removed

- `fufu.translate`, replaced by `fufu.gitPolicy`. Command translation is gone entirely: fufu will not run a different command than the one given.

### Fixed

- `ff trim` reported a live branch as gone when its operations had aged out of the keep window.
- With multiple worktrees, reconcile reported branch deletions and creations that had not happened, when another worktree held the branch.

### Known issues

- With signing enabled, `ff status` no longer predicts the next commit's sha.

## v0.9.0 — 2026-08-27

Hooks become one family. `ff hook`, `ff unhook`, and `ff trigger` serve all four integration clients (agent, shell, editor, git) from a single core, and the index is populated before hooks run, matching git's own order. fufu now ships an agent skill: `ff hook --skill` installs it, and the agent briefing routes to the skill instead of carrying the manual inline. `ff collide` now answers one pair of branches per invocation, and two lints that arrived with Rust 1.98 are resolved.

## v0.8.0 — 2026-08-25

Linked worktrees, done properly.

- Each worktree gets its own operation chain, records only the refs it owns, and a branch can be open in only one worktree at a time; a second worktree's first command is no longer treated as a first run.
- `ff worktree add`, `ff worktree remove`, and `ff worktree list` — making and taking a worktree are operations, `ff undo` reverses a worktree removal by capturing before it deletes, and the list shows the chains of worktrees that are gone.
- Retention and the survey reach every chain, including the ones nobody stands in, so a dead bay's work stays reachable.
- `ff watch` subscribes to the operation log, and `ff watch --all` streams every worktree in the repository on one tick.
- `ff collide` reports which branches would hit each other, backed by a new sideways-comparison axis in the core.
- `ff -C <dir>` runs fufu against another directory, and `ff <name>` reaches for an `ff-<name>` extension on PATH.
- `ff restack` moves only the branch you named, the pager scrolls again (the X left LESS), and worktree paths get one spelling across platforms including Windows.

## v0.7.0 — 2026-08-23

Remotes stop being invisible. `ff publish --to` names the remote and records it, `ff sync`'s fetch speaks the git protocol itself, and `ff doctor` and `ff status` now see the remote's state. `ff restack --onto` accepts a base that lives on a remote, `ff start` gains a park line and can fork from remote branches, a branch rename carries its upstream, and branch lookups stop guessing that the remote is origin. `ff -v` routes through the version verb, the README's console blocks are generated from real output, and CI shards the process-spawning test legs.

## v0.6.0 — 2026-08-21

The reading verbs and the remote verbs take shape.

- `ff diff` shows the open change as a patch, `ff show` renders one revision with header and patch, `-p` joins the three views that list files, and the tree diff reads down to the line.
- `ff commit <paths>` lands a slice of the open change and leaves the rest open; `ff log <paths>` filters the log by path.
- Sync and publish get one verb each: `ff sync` takes in, `ff publish` sends, publish remembers the push so sync stops reversing your undo, `--dry-run` previews, and each status count names the verb that clears it.
- `ff init` and `ff clone` make the starting point fufu's own; `ff version` replaces the uppercase flag.
- `ff history` shows the moves rather than the operations, `ff op log` shows every operation and takes a revset as its argument, and the op read verbs capture first so `@` means now.
- Git translation becomes opt-in via the translate setting, and merge, blame, and tag join the git words fufu answers.
- The map keeps the commits that relate branches, coded failures always point at a next step, the agent notice teaches fufu and the CLI holds agents to it, and CI runs green on all three platforms with a bench gate that stops coin-flipping.

## v0.5.0 — 2026-08-19

History rewriting lands as a verb family. A rewrite substrate arrives behind `ff describe <rev>`; `ff absorb` and `ff lift` are the short reach, `ff restack` on any branch is the primitive, and `ff edit` sessions are the long reach. Conflicts run on your schedule: rewrites that conflict are held for `ff resolve` instead of stopping you mid-flight. `ff sync` reports divergence on both axes and attributes it to the right side via the rewrite map, a branch's base and remote become one axis per ref, and `ff status` names the capture its parent row was cut from.

## v0.4.0 — 2026-08-17

Bare `ff` becomes the map: the repository's branches drawn as a skeleton, every branch name bolded, and the listing speaks the tool's dialect with color. Underneath, a branch now answers to a base and a remote, and futures arrive: `ff status` reports what syncing would cost — clean, conflict, or fast-forward — via a commit-by-commit replay probe, before you spend the operation. Short spellings land, and git-flavored words are answered with fufu's equivalent instead of parsed.

## v0.3.0 — 2026-08-16

The internal model consolidates and the machine surface lands.

- One log: the separate snapshot chain is retired, an operation is a log entry, and a snapshot is what an operation carries; the op log takes its own lock and stops growing with the commit log.
- Revsets: one set language and one grammar for both address spaces, git's `~`/`^` suffixes walk the log, operations evaluate the language too, and a hex-shaped restore target is an id, never a duration.
- The `ff op` family arrives and `ff undo` steps by runs; naming a branch is `ff describe -b`.
- Sessions become named spans of the capture chain — list them, diff them, group the log by them.
- The machine surface: every `--json` output carries a versioned envelope, errors carry stable ids and exit codes carry the verdict, and `ff explain` documents them; `ff status` computes one model that both renderings read.

## v0.2.0 — 2026-08-14

The first wave of daily-driver verbs and the presentation layer.

- New verbs: `ff start` begins new work on a fresh branch, `ff help` gets a written page for every verb, `ff doctor` verifies the net, `ff update` self-updates on two lanes, `ff config` speaks fufu.\* git config with a typed registry, and auto-trim enforces retention on its own.
- Change-centric `ff log`: an `@` open-change row over `●` commit rows with segment tips, plus `ff evolog` for the snapshot chain itself.
- jj-style reverse-hex snapshot ids with unique-prefix highlighting, a colored spine, and pager and color infrastructure gated on the TTY.
- A snapshot id index makes prefix resolution and the log family cost only the rows on screen, and `ff log` hops segments instead of walking the chain.
- A bench suite that measures slopes rather than milliseconds, with fixtures keyed to the binary that built them; `ff --version` names the build it came from; a cargo-deny gate enforces permissive-only dependency licenses.
- `ff status` shows one file list, one themed palette is chosen by `fufu.theme`, and usage lines say `ff` on Windows.

## v0.1.0 — 2026-08-13

The founding release, in three phases on top of the DESIGN.md founding document.

- Phase 0, bedrock: a native read core on gitoxide, read-only `ff status` and `ff log` with human and `--json` output, a permanent differential test harness against the real git binary, and a zero-spawn latency proof.
- Phase 1, capture: bare `ff` snapshots the working tree, plus the timeline, `ff restore`, `ff trim`, git passthrough, and agent hooks.
- Phase 2, time: an operation journal with whole-repo `ff undo`, tree memory, and `ff commit`, `ff switch`, `ff new`, `ff branch`, `ff describe`.
- Integrations unify under `ff hook <agent|shell|editor>`, and release scaffolding lands: LICENSE, README, installers, CI and release workflows, and Windows portability gates.
