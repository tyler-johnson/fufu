# Changelog

## v0.10.0 — 2026-08-31

A documentation site. Everything fufu knew about itself lived in DESIGN.md, a README walkthrough, and forty-nine `--help` pages — nothing a newcomer could read front to back. [The site](https://tyler-johnson.github.io/fufu/) is thirty-two hand-written pages organized jj's way — getting started, concepts, guides, reference, comparisons, internals — plus a section jj has no need for: for agents. The tutorial walks the whole loop, five guides cover recovery, rewriting, stacked changes, worktrees and plain-git teammates, and every console block on the site is pasted from a script under `scripts/docs/` that ran against the real binary, so a changed output is one invocation to regenerate. The README stops demonstrating and points at it.

Two parts of the site generate themselves. The forty-nine help pages moved out of `help.rs` into markdown under `crates/ff-cli/src/help/`, rendered to seventy-two columns for the terminal on every invocation — so `docs/reference/cli/` is fifty-one pages emitted from that same source under a byte-equality test, and CI fails the moment the reference drifts from `--help`. The config key registry is generated the same way. `ff --help` also stopped being forty commands in one alphabetical block: the root page groups them under `git help`'s own headings, and `ff -h` shows fourteen common verbs, git's `help` against `help -a` split.

`fufu.gitPolicy` replaces `fufu.translate`. Translation was a boolean, off by default, that made `ff git commit` quietly run `ff commit` instead — the one thing a correction mechanism must not do, run a different write command than the one asked for. That execution path is deleted rather than re-gated. Three tiers stand in its place: `observe` records and says nothing, `coach` (the default) names the fufu alternative the first time a git word comes up, `strict` refuses the words fufu has verbs for and says what to run instead.

Both entry points obey it — `ff git`, which is what a person types through the alias, and a raw `git …` inside an agent's Bash tool, which `PreToolUse` sees. Three rules keep it safe: fufu never runs a write verb you did not type, only the eighteen git words fufu has an answer for are ever touched, so `git apply`, `git bisect` and `git gc` keep `ff git` an honest escape hatch, and anything that is not one plain `git <word> …` invocation fails open. `ff doctor` gains a row reading the tally. The trigger contract's never-vetoes clause is amended where it sat: a hook never vetoes on its own judgment, and vetoes only where config said to.

The briefing reaches every audience. A subagent inherits its parent's session id and fires no prompt event, so under a marker holding one session per client it worked with no idea fufu was there. The marker now records which audiences were briefed inside a session, and a tool call briefs whoever is making it — which is also what reaches a repository the agent has just `cd`'d into. `SessionStart` becomes its own event and re-briefs across a resume, a `/clear` or a compaction, and Claude Code's plugin gains `Stop` and `SubagentStop` as pure capture, so the file state an agent writes as the last thing in a turn is snapshotted then rather than waiting for whatever comes next.

Commit signing. fufu writes commit objects itself and gitoxide implements no signing, so `commit.gpgsign` was read nowhere: `ff commit` minted unsigned commits whatever git config said, and every rewrite verb stripped the signature it found with nothing to put back. fufu now honors git's signing configuration exactly — `commit.gpgsign`, `gpg.format`, `user.signingkey` and the program keys — in all three formats git signs in: openpgp through gpg, x509 through gpgsm, and ssh through ssh-keygen. Nothing new was added to the `fufu.*` registry; these are git's keys, in git's spelling.

`commit.gpgsign` governs every user commit fufu writes, replays included — a departure from git, where `git rebase` needs `rebase.gpgSign` said separately and quietly unsigns a branch without it. `ff commit` gains `-S` and `--no-sign` for the one-off; rewrite verbs take the setting, with git's own `GIT_CONFIG_*` escape hatch for overriding one invocation. Op-journal and park commits are never signed: the first is not your work, the second never leaves the repository.

Signatures are read back where commits are shown. `ff log` and `ff status` mark a signed commit `signed`, by default and for free — carrying a signature is a header on an object the walk already read, so no signer runs; the word is `signed` rather than `verified` because nothing was checked to say it. `ff log --signatures` does check, replacing the mark with the verdict, the tool and the key's short id — `verified gpg 9B295D68`, or `bad signature`, `untrusted key`, `expired key`, `revoked key`, `unverifiable`. Those runs go in parallel, one per core up to eight, which is worth about 2.6x on a page of twenty; signing stays sequential, since a passphrase prompt cannot be raced. An unsigned commit is marked nothing either way. `ff show` verifies the single commit it prints and says so, and `ff doctor` has a `signing` row that reports whether the setup will work without running anything. One accepted regression: with signing on, the `@` row shows no predicted sha, because the signature is not knowable without running the signer.

Trim stops calling a live branch gone. A pointer is deleted for two unrelated reasons — the branch no longer exists, or every operation behind it aged out of the keep window — and the report used one wording for both, so a branch that was alive and well printed `branch is gone`. The report now records whether `refs/heads/<branch>` exists, and the wording follows it: a gone branch says so, a live branch says its operations aged out and the branch itself is untouched.

Reconcile stops fabricating branch reports across worktrees. A branch another worktree holds is invisible to observation on purpose, and the stored ref table used to drop it — so taking a branch elsewhere printed `refs/heads/<branch> deleted` while it lived, and releasing it printed `created at`. The table now carries held-elsewhere entries forward at their last-known sha, so the board only reports motion that happened. No migration: old logs read as-is, and the delete direction is fixed immediately even against a pre-upgrade baseline. A baseline written while a branch was hidden genuinely lacks the entry, so its release prints one `created at` line once — the same as before the fix, and self-healing.

Three files that had grown past comfort became directory modules with no logic changed: the core's `rewrite.rs` into replay, chain and markers; the CLI's `render.rs` into palette, age, status, diff and rows; `doctor.rs` into repo, wiring and render. The docs site deploys itself from its own workflow, and a docs-only push no longer spins the rust matrix.

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
