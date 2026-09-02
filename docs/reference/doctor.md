# Doctor

A safety net you cannot inspect is not trustworthy, and every floor of fufu's can degrade quietly: a log ref moved by something that is not fufu, a reflog that never got created, the gc guard deleted out of local config, a branch that answers to no remote anything can name, hooks never installed, a stale binary. `ff doctor` reads the whole net in one pass and prints one row per check. It observes and never enforces — no snapshot is taken, no drift is absorbed, nothing is reconciled. The one consented write is `--fix`, covered [below](#the-one-write-fix). The flags and examples live on the [CLI page](cli/doctor.md); the wiring it verifies is what [agent setup](../agents/setup.md#verify) installs.

## Verdicts

Every row comes at one of three levels:

- `ok` — the check passed. It counts for nothing beyond that.
- `info` — news rather than a problem: a fact worth knowing (the last operation, a non-default setting, refs a delete deliberately left behind) that requires nothing from you.
- `WARN` — a finding. Something in the net has degraded, and the row says what and names the repair.

Findings drive the exit code — 0 healthy, 1 findings — so CI can gate on it, and `--json` emits the same rows for machines, with `findings` and `fixable` counts alongside the checks.

## A healthy run, annotated

This is a real run in a healthy repository:

```console
$ ff doctor
  ok    repository     ~/Development/fufu/.git
  ok    log            refs/fufu/wt/main/ops, newest operation 3m ago
  ok    identity       the log tip is a fufu operation
  ok    pointers       3 branch pointer(s) into the log: ci-micro-probe 6d ago, main 3m ago, worktree-readme-describe-b 1w ago
  ok    reflogs        the log ref has a reflog — undo and redo have somewhere to record where the pointer has stood
  ok    gc config      reflog expiry disabled for refs/fufu/*
  ok    objects        1369 loose, 3 packs
  ok    id index       2779 ids, in sync
  info  last op        "published main to origin/main" 3m ago
  info  legacy         3 ref(s) under refs/fufu/legacy/ hold snapshots and operations from before the one-log cutover; this fufu cannot read them, and they are kept only so nothing was destroyed silently. Delete them with git when you no longer want them.
  info  settings       gitPolicy strict
  info  trim           nothing to drop — every operation is inside the keep window
  info  auto-trim      last ran 3h ago (at most every 1d)
  ok    remotes        1 configured (origin) — every branch names one
  info  upstreams      config for ci-windows-sharding names no branch here — the shared copy is still on the remote, which is what `ff branch delete` leaves behind
  ok    claude         plugin wired in ~/.claude/skills/fufu
  ok    codex          settings wired in ~/.codex/hooks.json — Codex trusts a hook by its hash: run /hooks in Codex to review this one, or it is skipped and nothing captures
  ok    alias          git='ff git' wired in ~/.bashrc (`ff hook bash` manages it)
  ok    ambient        prompt hook snapshots at every prompt, wired in ~/.bashrc (`ff hook bash` manages it)
  ok    skill          fufu's manual, for claude, codex
  ok    mcp            registered with claude, codex
  info  update         source build — updates via cargo install

no findings — the net is under you
```

The rows group into four floors: the engine, the remote floor, the wiring, and the update lane. The lanes below cover everything doctor can report, including rows this healthy run had no occasion to print.

### The engine

**repository** — where the `.git` directory is. A bare repository or a directory outside git is `info`, and the repo checks are skipped: there is nothing to snapshot, so there is no net to read.

**log** — the operation log's ref and the age of its newest operation. One log, so one row; a repository where the engine has never run gets a `WARN` here instead (see [common failures](#the-engine-has-never-run-here)).

**identity** — whether the log tip is a fufu operation. Anything else means the ref was moved by something other than fufu, which is a `WARN`.

**pointers** — the per-branch refs that point *into* the log, one per branch with the age of its newest operation. A pointer naming an operation the log does not hold is what fufu's two-ref append exists to prevent, so healthy pointers are worth a row.

**reflogs** — whether the log ref has a reflog. It is load-bearing: `ff undo` steps the ref back rather than appending, so where the pointer has stood is recorded only here — it is what `ff redo` walks forward along and what keeps an abandoned branch of the log addressable. No reflog is a `WARN`.

**gc config** — whether reflog expiry is disabled for `refs/fufu/*` in local config. Without those keys, a manual `git gc` could expire fufu reflog entries. Missing keys are a `WARN`, and the one `--fix` writes.

**trash** — `info`, only when present: pre-trim tips held until the next trim.

**objects** — loose object and pack counts. fufu writes objects natively and never triggers git's auto-gc on its own, so once the loose count passes `gc.auto` the row turns `info` and points at `ff trim`, which nudges git to pack them.

**id index** — the index behind short operation ids. Read-only like everything else here: a stale or absent index is `info`, because both self-heal on the next `ff log` or `ff evolog`.

**last op** — `info`: the newest operation's summary and age. A tip that does not parse as an operation is a `WARN` (it accompanies the identity warning when the ref was moved).

**drift** — `info`, only when present: refs moved outside fufu since the last operation, absorbed on the next one. Doctor reports the drift and deliberately does not absorb it — that would be the observer changing what it observes.

**legacy** — `info`, only when present: refs under `refs/fufu/legacy/` holding snapshots and operations from before the one-log cutover. This fufu cannot read them; they are kept so nothing was destroyed silently, and you delete them with git when you no longer want them.

**parked** — `info`, only when present: branches whose tree memory `ff switch` is holding.

**settings** — every fufu key in config, validated through the same parsers the readers use. Defaults and valid non-default values are `info`; a value the reader cannot parse is a `WARN` naming the key and pointing at `ff config <name>`.

**trim** — a dry-run preview: how many operations have aged past the keep window, and that `ff trim` would drop them. Always `info` — old operations are the trim schedule's business, never a finding.

**auto-trim** — whether the automatic trim is on, its cadence, and when it last rode an ff command. Always `info`.

### The remote floor

**remotes** — only in repositories that have remotes at all; a local-only repository has no remote floor and no finding. Every branch must be able to name the remote it answers to. A branch that cannot — typically two remotes and nothing choosing between them — is a `WARN`, because `ff sync` and `ff publish` will both refuse until `ff publish --to <remote>` chooses one.

**upstreams** — `[branch "<name>"]` config sections naming branches that are not here. Two cases, deliberately kept apart. A section whose shared copy still exists on the remote is `info`: that residue is what a plain `ff branch delete` of a published branch leaves behind, on purpose, so undo stays exact. A section pointing at nothing on either side is drift, a `WARN`, and the other thing `--fix` repairs.

**tracking** — branches that exist here but whose upstream's shared copy is gone. `info`, because `ff status` already reports `remote is gone` for the branch underfoot; repo-wide it is news.

### Raw git

**raw git** — `info`, only when the [git policy](../agents/setup.md#pick-a-git-policy) has counted something: how many raw git writes this repository has seen, how many were refused, and when the last one happened. Under the `observe` tier it adds the nudge that tier exists to earn:

```console
  info  raw git        1 write(s), last 0s ago — `ff config gitPolicy coach` names the alternative
```

Silent when nothing has been counted — a row saying zero would be a row about a thing that never happened.

### The wiring

These rows come from the same status vector `ff hook -l` renders, so the two commands cannot disagree about what is wired.

**One row per agent client** (`claude`, `codex`, …) — `ok` when wired, naming where the hook landed; [the hook reference](hooks/index.md) shows what lands there. A client that is present on the machine yet unwired is `info` with the `ff hook <slug>` that would wire it; a client that is neither present nor wired earns no row, because a client you do not have is not a hole in the net. Two states are findings: wired but with an event missing (capture is partial), and wired in a spelling this fufu no longer writes. Both are repaired by `ff hook <slug>`, and both are among the things `--fix` rewires.

**alias** — whether `git='ff git'` is wired in a shell rc file, folded across the shells: one shell wired answers the question. A hand-written alias is `info` (heuristic — check `type git` in your shell), never a finding.

**ambient** — the prompt hook that snapshots at every prompt, reported separately from the alias because the shells wire the two pieces independently.

**skill** — fufu's shipped manual, aggregated across the clients that read one. Absence is never a finding: without the skill an agent is down to the once-per-session briefing, which costs it spelling and not file state. Drift is the one thing worth a `WARN`, because a manual describing a fufu that has moved teaches commands that fail.

**mcp** — the [`ff mcp`](cli/mcp.md) server's registration, aggregated across the agent clients. `ok` names the clients that have it; `info` when none does, because an agent without it shells out to `ff` and loses nothing but a typed tool. The one `WARN` is a client whose hook is wired and whose server is not, the shape an install predating the server leaves, and `--fix` runs that client's installer again.

**triggers** — the one finding about the whole net rather than any piece of it. When nothing at all feeds capture — no agent hook, no alias, no prompt hook, not even a hand-written line — doctor warns that snapshots only happen when you run `ff` by hand, and points at `ff hook`. A silent engine feels safe while capturing nothing.

### The update lane

**update** — always `info`: up to date, an available version and the `ff update` that fetches it, a source build that updates via cargo, or no check yet.

## Common failures

Every transcript below is real output from a deliberately broken repository.

### The engine has never run here

A git repository fufu has never touched — cloned before the hooks were wired, or adopted on a machine without them:

```console
$ ff doctor
  ok    repository     ~/scratch/unarmed/.git
  WARN  log            no refs/fufu/wt/main/ops — the engine has never run here (run `ff`, or any git command via the alias)
  ...
```

The fix is the row's own suggestion: any fufu command opens the log and takes the first snapshot. [Adopting a repository](../adopting.md) covers what that first operation records.

### The gc guard is gone

Someone or something removed the `gc.refs/fufu/*` keys from local config:

```console
  WARN  gc config      gc.refs/fufu/*.reflogExpire{,Unreachable} not `never` — a manual `git gc` could expire fufu reflog entries (--fix writes them)
```

`ff doctor --fix` writes the keys back and reports it in the same row:

```console
  ok    gc config      reflog expiry disabled for refs/fufu/* (fixed)
```

### The log ref was moved by something other than fufu

A raw `git update-ref` on the ops ref, a botched script, a tool that rewrote `refs/fufu/*`:

```console
  WARN  identity       the log tip is not a fufu operation — the ref was moved by something other than fufu
  WARN  last op        the log tip does not parse — 2d073899264443d6ccbe0edb19cd76a47ad81154 is not a fufu operation
```

The reflog holds where the tip stood, so the repair is stepping the ref back to its last recorded position:

```console
$ git update-ref refs/fufu/wt/main/ops refs/fufu/wt/main/ops@{1}
$ ff doctor
  ...
no findings — the net is under you
```

### The log ref has no reflog

```console
  WARN  reflogs        the log ref has no reflog — ff redo cannot walk forward, and --at cannot answer questions about where the log has been
```

The next fufu operation recreates the reflog and the row goes back to `ok`; entries accumulate again from that point. What stood in the missing reflog is gone — `ff redo` and `--at` cannot reach positions that were only recorded there.

### Config naming a branch that is gone from both sides

```console
  WARN  upstreams      config for feature-x names no branch here and no tracking ref either — `ff doctor --fix` removes the section
```

`--fix` removes exactly these sections and no others. A section whose shared copy is still on the remote stays untouched — that residue is `ff branch delete` doing its job, and the `info` variant of this lane says so.

### The wiring drifted

A skill written by an older fufu, a hook stored in a retired spelling, a client wired with an event missing:

```console
  WARN  codex          an older fufu wrote the skill in ~/.codex/skills/fufu (`ff hook codex` repairs)
```

`ff hook <slug>` rewires it, and `ff doctor --fix` does the same thing in passing. This is why the wiring repair lives in doctor at all: a stored string is only rewritten when somebody runs the installer again, and doctor is the command people run when they are already suspicious.

## The one write: --fix

Read-only is the design, because doctor must never absorb the drift it reports. `--fix` is the one consented write, and it repairs exactly the findings whose rows say so: the gc reflog-expiry keys, a `[branch]` config section that names nothing on either side, and wiring stored in a retired or partial spelling. Everything else — a moved log ref, a missing reflog, an invalid setting — is reported with the repair named in the row, and the repair stays yours to run. The summary line counts what `--fix` would take:

```console
2 finding(s) — `ff doctor --fix` repairs 1 of them
```

## When to run it

- **After adopting a repository** — the engine floor confirms the log opened and the gc guard is in place, and the wiring floor confirms something actually feeds capture. [Agent setup](../agents/setup.md#verify) runs it as the verification step.
- **After a version bump** — the update lane confirms which binary answered, and the wiring rows catch hooks and skills written by the fufu you just replaced.
- **When something feels off** — an `ff undo` that did less than expected, a branch that will not publish, an agent whose edits are not showing up in `ff history`. One pass names the floor that degraded.
- **In CI** — the exit code gates: 0 healthy, 1 findings, and `--json` gives the pipeline the rows.
