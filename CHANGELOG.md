# Changelog

## Unreleased

### Added

- `ff evolog <rev>` drills into a change: every operation, on any worktree's chain, that produced a commit carrying its change id, then the captures behind the close. `--json` carries `change_id`, `commit`, `operations`, and `snapshots`.

### Changed

- The letters column on `ff log`, `ff status`, the map, and `ff show` is a change id, the identity a commit keeps through every rewrite fufu performs, in place of the op anchor. `ff commit` writes it as jj's `change-id` header inside the signed payload, and a commit made outside fufu derives one from its sha, so the column is never blank. `--json` carries `change_id` beside the ids it carried before.
- A letters token in a revision slot is a change id or a unique prefix of one, and a prefix of the open change's id is `@`. An ambiguous prefix is `usage/revset-ambiguous`; a change standing on more than one visible commit is `usage/revset-divergent`. Operation ids are hex now and keep their own slots.
- Operation and capture ids print and parse as hex, twelve characters in every column, like jj's: `ff op log`, `ff evolog`, `ff history`, `ff undo`, and the bold prefix `--at-op` reads, and every `id` the JSON surface carries for an operation. Letters are a change id and nothing else: `ff op show <change id>` and `--at-op <change id>` are `usage/rev-in-op-position`, and `ff log -r <op id>` is `usage/op-in-rev-position`.
- `ff switch <change id>` redirects to `ff start` at that commit, the way `ff switch <sha>` does.
- `ff restack` and the cascade drop a commit whose change id the base already holds, reflog or no reflog: `dropped … — superseded by <sha> in the base`. JSON `dropped` entries gain `reason` and, under `superseded`, `by`.
- A branch created outside fufu records the branch it was cut from as its base when its tip is exactly one other non-trunk branch's tip or git's reflog names it. The absorb line says `forked from <branch>`, and `ff undo` takes the record back. Anything less certain stays on trunk.
- A git upstream under another local branch's name, or trunk's, is the branch's base rather than its shared copy: `ff status` shows it on the base axis, `ff branch list` shows no copy, and the `branch/aliased-copy` refusal goes. An upstream under a name no local branch holds is still the branch's own copy.

### Removed

- `id_letters` from `ff status --json` and `ff log --json`; `id` is the operation's hex, and what `--at-op` reads.

### Fixed

- `ff commit -b <fresh>` from a named branch no longer leaves its pending description on the branch it left.
- `ff restack --onto` trims the replay by the target's reflog, not only the recorded base's, so a branch cut outside fufu no longer replays a stale copy of a commit its base has since rewritten. (#5)
- `ff push` on a branch whose upstream is another branch's tracking ref creates the branch's own copy and records the upstream as its base, instead of pushing to that ref under a lease. (#7)

### Known issues

- A rebase or cherry-pick run outside fufu drops the `change-id` header, and the commit comes back with a derived id. jj has the same limitation.
- A capture's mint is not journaled: after `ff undo` and `ff redo` of a partial close, the remainder can wear a different id than it did between them.

## v0.13.0 — 2026-09-08

### Added

- `ff pull <branch>...` and `ff push <branch>...` act on the branches named, from wherever you stand: a pull brings each with the local bases beneath it, a push sends each under its own lease, and a lease the remote refuses is that branch's alone. A name resolves the way `ff restack` resolves one. `ff pull --all` is every local branch; push has no `--all`.
- `ff pull --dry-run` (`-n`) says which branches would fast-forward, replay, hold, or be skipped, and writes nothing but remote-tracking refs; `--no-fetch` beside it reads what you already have. The exit is 3 when a branch would hold.
- `ff status --json` carries the orientation an agent asks for first: `root`, `worktree`, `base` with the count above it, `remote`, and `last_op`.
- Every operation carries its route, `shell` or `tool`, in its trailer: on `ff op show` and `ff op log --json`, and as the `route()` filter in the op log's set language.
- Every `ff` invocation reads `CLAUDE_CODE_SESSION_ID` when neither `--session` nor `FF_SESSION` is set, so a shell verb under Claude Code carries the session its hook captures do; `ff mcp` reads it once at start.
- jj's names as aliases: `ff bookmark`, `ff workspace`, `ff squash`, and `ff rebase` run `ff branch`, `ff worktree`, `ff absorb`, and `ff restack`. `ff abandon` and `ff split` answer with `usage/foreign-verb` naming the moves that cover them. Every alias shows on `ff --help`.
- The manifest's `update` and `build` fields. `ff update` walks every declared extension after fufu by the rules it applies to itself, the passive release notice covers an `official` build with a github.com `releases` recipe, and `ff extension add` says how `ff update` will answer for the extension. `update.json` carries `extensions`.
- `ff hook -u` refreshes what is wired and adds nothing: every declared extension's manifest is re-asked, then the install re-run for every slug already wired. `install.sh` and `install.ps1` run it after placing the binary. `ff hook --json` carries `extensions`.
- `ff doctor`'s Extensions floor reports a declared extension whose binary has moved past its record as that extension's own drift row, with `ff extension add <name>` as the repair.

### Changed

- `ff sync` is `ff pull` and `ff publish` is `ff push`; the old spellings stay as visible aliases. The error ids under `sync/` and `publish/` are `pull/` and `push/`, and the old ids resolve nowhere, `ff explain` included. The operation log, the JSON envelope's `cmd` and `data` key, and the counts `ff status` prints (`N to pull`, `N to push`) follow.
- Bare `ff pull` is the branch you stand on and the local bases beneath it, rather than every local branch; `ff pull --all` is the whole-repository run.
- `ff mcp` serves seven typed tools, `status`, `pull`, `push`, `undo`, `redo`, `explain`, and `help`, each taking the verb's own flags as fields and a `cwd`, in place of the one `ff` tool and its args array; every other verb is the shell. `isError` follows the envelope rather than the exit code, and every result carries the child's exit in `_meta.exit`. The briefing names the tools only where the client has `ff mcp` registered.
- `ff resolve` opens a session branch the way `ff edit` does, with the hold staying on the branch you left; `ff done` lands the fixes and returns in one operation, and `ff status` carries the session under `resolving` on both branches.
- The manifest's `skills` field names skills, and `ff hook` asks `ff-<name> --ff-skill <skill>` for each one's files, installed whole beside fufu's own; a failed handshake is `extension/skill-failed` or `extension/bad-skill`. A registry record from before reads as unreadable until `ff extension add <name>` rewrites it. `undoable` and `verbs[].read_only` are informational.
- `ff update -y` is one answer for the whole walk: a channel it cannot drive is named at the end and the exit is 1 there, after every extension has been reached.
- "Working copy" replaces "working tree" everywhere fufu speaks.
- Changes made outside fufu render as one summary line in the reconcile preamble and the block `ff status` pins; `ff status --json` still carries every ref.
- A `git rebase` typed through the shell alias is coached toward `ff restack` under `fufu.gitPolicy=coach` and refused under `strict`.

### Removed

- `fufu.toolPolicy`, with the presence marker `ff mcp` held for it and the `ff trigger claude` refusal it drove.
- `usage/mcp-verb-unavailable`, `usage/mcp-extension-undeclared`, `usage/mcp-extension-not-undoable`, and `usage/mcp-policy-write`, with the sealed keys under the tool.
- `ff co`, the hidden alias on the `checkout` foreign verb.

### Fixed

- `ff switch`, `ff done`, and the resolution landing recorded an end tree without the untracked files of the park they resumed, so the next `ff undo` deleted the files instead of stepping back.
- `ff switch` away during a resolution no longer overwrites the branch's parked change with the marker tree.
- `ff pull`, `ff restack`, `ff absorb`, and every other replay walked a range by commit date as well as ancestry, so a commit dated older than the base was left out of the replay, and a branch made only of such commits was refused as already sitting on its base. In a shallow clone the walk stops at the boundary.
- `ff extension remove` before `ff hook claude` no longer leaves the extension's skills in the plugin. The Codex half of the v0.12.0 known issue stands.

## v0.12.1 — 2026-09-04

### Fixed

- Bare `ff` counts a parked change's untracked files, so the map's `(+ parked change, N)` and the number `ff switch` prints on arrival agree.
- The agent skill described `ff restack`, `ff sync`, and held rewrites as they were before the cascade, so an agent was told a conflicted replay left nothing changed and that `ff sync` covered one branch.
- The refusal for an undeclared extension names `ff <name>` as the command to run, where it showed only the `ff-<name>` binary on PATH and left the shell exemption to `ff explain`.

## v0.12.0 — 2026-09-04

### Added

- The cascade: `ff restack`, `ff sync`, `ff absorb`, `ff lift`, `ff describe <rev>`, and `ff done` replay the branches stacked on the one they moved, inside the same operation. A branch that conflicts is held on its own; `ff restack` and `ff sync` exit 3 when any did.
- `ff sync` covers the whole repository: one fetch, then every local branch is brought up to date with its shared copy and its base, parent before child.
- `ff mcp`, a Model Context Protocol server on stdio serving fufu's verbs as one tool. `ff hook <client>` registers it beside the capture hook.
- `fufu.toolPolicy`: what fufu says when an agent runs `ff` in its shell while that tool is up. `observe`, `coach`, or `strict`, the default.
- Extensions can declare themselves. `ff extension add <name>` records what `ff-<name>` says it is, and fufu then describes that verb to an agent everywhere it already speaks. `docs/reference/extensions.md` is the reference.
- `ff config` refuses a write to `fufu.gitPolicy` or `fufu.toolPolicy` through the `ff` tool, so an agent cannot lower the tier policing it.

### Changed

- `ff <verb> --help` is shorter, and the pages take subheadings and lists.

### Fixed

- `ff restack` and `ff resolve` on a branch whose base was rewritten replay the branch's own commits alone, bounded at the fork point rather than at the merge base with the rewritten tip.

### Known issues

- A subscriber's injected `context` is uncapped, where fufu's own briefing line is dropped past 240 characters.
- An extension's tool list is held for the life of the connection, so a client restart is what picks up an edited one. Two produced tools can collide on one name, and the later is dropped without a word.
- `ff doctor` compares a declared extension's recorded version, so an edit without a version bump drifts unreported. A changed `mcp.args` is reported stale and not repaired by `--fix`.
- `ff extension remove` before `ff unhook` leaves that extension's skills directory and its Cursor and Gemini registrations behind.

## v0.11.0 — 2026-09-02

### Added

- The error id index, `docs/reference/errors.md`: every id `ff explain` knows, with its exit code and one-line meaning, generated from the registry. `ff explain --json` entries carry `exit`, the code the id exits with, and `ff explain --list` prints it as a column.
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
