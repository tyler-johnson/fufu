# Agent setup

[Why agents want fufu](why.md) is the argument; this page is the wiring.

Pick a git policy, install the hooks and skill, and verify capture in a scratch repository. Standing orders can also live in project instructions. The verification includes recovering a captured edit with [`ff undo`](../reference/cli/undo.md).

[`ff hook --all`](../reference/cli/hook.md) installs integrations for detected clients and shells, including the briefing and supported skills. Commands run through the shell; fufu no longer serves an MCP tool. The blocks below show the wiring for manual installations.

## Pick a git policy

`fufu.gitPolicy` decides what fufu says when git is reached for directly — through the [`ff git`](../reference/cli/git.md) passthrough, or in the agent's own shell through the hook. Three levels:

- **`observe`** records the git word and says nothing. Capture still runs first; you get the tally without the commentary.
- **`coach`** — the default — injects one line naming the fufu verb the first time each git word comes up: `tip: that's ff commit`. An agent reads the correction as an instruction, so this is usually enough.
- **`strict`** refuses a git write that has a fufu verb and names what to run instead. Through the hook the refusal travels as JSON (`permissionDecision: deny`); on the alias it is exit 2.

Set it per repository with [`ff config`](../reference/cli/config.md), which validates the value before writing it:

```console
$ ff config gitPolicy strict
```

For an agent you are watching, `coach` is the right default. One correction is cheap, and the agent adjusts. For an unattended or long-running agent, set `strict` — the fufu repository itself runs under it.

Know what strict does not promise before you lean on it. Git words with no fufu verb pass untouched, ambiguous shell strings fail open, and `ff git <args…>` stays an open escape hatch. [why.md walks the limits](why.md#strict-mode-as-a-leash).

An allowed passthrough attempts capture before running Git, but capture can fail with a warning. A strict passthrough refusal happens before capture. Agent hooks attempt capture before evaluating policy; their denials rely on the client enforcing the response.

## Standing orders: the CLAUDE.md / AGENTS.md block

Paste this into your project's `CLAUDE.md`, `AGENTS.md`, or whatever memory file your client reads. It is the same text fufu's own briefing carries:

```markdown
## Version control

fufu (`ff`) takes snapshots through repository commands and active hooks. Recovery requires a successful, retained capture; ignored and oversized files may be excluded.

Use `ff`, not `git`, for anything that writes. `ff commit -m "…"` closes the open change — no add, no staging, the working copy is the change. `ff switch <branch>` moves. `ff undo` takes back the last undo step in this worktree. `ff restore <path>` discards a file's edits. Anything else git does: `ff git <args…>`, which attempts a snapshot before running permitted git commands verbatim.

Reading with git is fine. `ff status`, `ff log`, and `ff diff` say more than their git counterparts.

Every verb's own `--help` is the authority on it.
```

With the hook below wired, fufu injects this briefing itself: at the turn boundary, again after anything that rebuilds the context (a resume, a `/clear`, a compaction), and once for each subagent.

The file copy is for clients without a hook channel, and for repositories where you want the doctrine standing in project memory regardless of which machine the agent runs on. Having both costs a few lines.

## The per-turn hook

The hook attempts capture on the events the client sends. It must be installed, trusted where required, and active in that session. [Snapshot coverage and limits](../concepts/snapshots-and-undo.md#coverage-and-limits) apply. One command installs it:

```console
$ ff hook claude
```

The slugs are `claude`, `codex`, `cursor`, and `gemini`, plus `bash`, `zsh`, `fish`, and `powershell` for the shell alias and prompt hook. Bare [`ff hook`](../reference/cli/hook.md) reports what it detects and asks; `ff hook --all` takes everything without asking.

For Claude Code the installer writes a plugin directory fufu owns outright. For the other clients it merges entries into their own settings file, and it never touches a line you wrote yourself. [The hook reference](../reference/hooks/index.md) shows the files each slug writes and what [`ff unhook`](../reference/cli/unhook.md) removes.

If you manage your Claude Code settings by hand instead, this is the wiring — the pattern the fufu repository itself runs under, pasted into `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Edit|Write|NotebookEdit",
        "hooks": [{ "type": "command", "command": "ff trigger claude" }]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [{ "type": "command", "command": "ff trigger claude" }]
      }
    ]
  }
}
```

These two events provide capture before the matching tools and at the turn boundary. `PreToolUse` also reaches subagents and newly entered repositories when the client emits it. `UserPromptSubmit` is the turn boundary the briefing rides. Hook captures are best-effort, including when another writer holds the lock.

The installer wires five more (`SessionStart`, `Stop`, `SubagentStop`, `SubagentStart`, `CwdChanged`) that widen capture rather than found it. The `Stop` pair matters most: capture is snapshot-*before*, so without a turn-end event, whatever the agent writes as its final action sits uncaptured until the next thing happens.

### The same wiring for Codex

`ff hook codex` wires the same floor into Codex. It merges two events into `~/.codex/hooks.json` — `PreToolUse` on the tools that mutate, and the turn boundary — and writes [the skill](#ship-the-skill) to `~/.codex/skills/fufu/`. This is what lands:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|apply_patch",
        "hooks": [{ "type": "command", "command": "ff trigger codex" }]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [{ "type": "command", "command": "ff trigger codex" }]
      }
    ]
  }
}
```

Two honest differences from the Claude Code wiring:

- **The floor and nothing wider.** The turn-end and subagent events the Claude Code installer adds have no counterpart here today, so whatever an agent writes as its final action waits for the next turn's capture.
- **Codex trusts hooks by their hash.** After `ff hook codex`, run `/hooks` inside Codex once to review and accept the new hook, or Codex skips it and nothing captures. `ff doctor` keeps showing that reminder whenever the wiring is present, because fufu cannot read Codex's trust list — an unreviewed hook and a reviewed one look identical from outside.

[`ff trigger <source>`](../reference/cli/trigger.md) is built for this seat. It reads the client's payload on stdin, always exits 0 whatever went wrong, and never vetoes a tool call on its own judgment.

The one veto that exists, `fufu.gitPolicy strict` for raw git, is config saying so, and it travels as JSON the client enforces.

A source name fufu does not know exits 0 silently, so the same command is safe to wire into a client fufu has never heard of. When your agent is a script rather than a client with hooks, [the machine surface](machine-surface.md) is the contract to build against.

### Two agents in one repository

This is a supported shape, and the wiring above is all it takes.

- **A worktree each.** Give every agent its own with [`ff worktree <path>`](../reference/cli/worktree.md). Each has its own operation chain and capture lock; shared refs can still contend. `ff undo` follows the current worktree's chain, subject to guards on branches held elsewhere.
- **One worktree shared.** Two agents there settle at that chain's lock instead. A capture that loses the lock is skipped, because the winner is already recording. A verb waits briefly and then refuses with `ref/contended`, exit 4, rather than interleaving — run it again.

One chain is also one undo. `ff undo` takes back the last operation whoever wrote it, so undoing agent A's mistake after agent B has moved on takes back B's work first.

The scoped verb is [`ff op revert <op>`](../reference/cli/op-revert.md), which inverts one operation and leaves later ones standing, where the refs it moved have not moved since. [Two writers on one chain](../guides/recovery.md#two-writers-on-one-chain-and-only-one-was-wrong) in the recovery cookbook walks it, and the [worktrees guide](../guides/worktrees.md#two-writers-one-repository) has the full story.

## Ship the skill

The briefing is deliberately short — four verbs, the git rule, and a pointer to `--help` — because the agent pays for it every session.

Everything past that lives in a skill fufu ships: the recovery table, rewriting commits that have already closed, held rewrites and conflicts, the landmines, and the JSON surface. It costs the agent nothing until the situation calls for it.

That skill is the difference between an agent that reads [`ff evolog`](../reference/cli/evolog.md) to find a lost hour and one that improvises reflog archaeology through `ff git`.

`ff hook claude` and `ff hook codex` install the skill beside the wiring — [the Codex subsection above](#the-same-wiring-for-codex) says where — so there is nothing extra to do on those clients. For a client that reads no skills directory, print it and put it wherever your agent reads instructions:

```console
$ ff hook --skill
```

## Verify

[`ff doctor`](../reference/cli/doctor.md) reads the whole net in one pass, and its wiring lane is the part this page set up. Healthy rows name where each hook landed:

```console
$ ff doctor
  ok    log            refs/fufu/wt/main/ops, newest operation 2m ago
  ...
  info  settings       gitPolicy strict
  ok    claude         plugin wired in ~/.claude/skills/fufu
  ok    skill          fufu's manual, for claude
  ok    alias          git='ff git' wired in ~/.bashrc (`ff hook bash` manages it)
  ok    ambient        prompt hook snapshots at every prompt, wired in ~/.bashrc (`ff hook bash` manages it)
```

A half-wired client is a `WARN` naming the missing event, and `ff hook <slug>` repairs it; so does `ff doctor --fix`, which runs the installer again.

When nothing at all feeds capture, doctor warns about that too, because a silent engine feels safe while capturing nothing. Findings drive the exit code — 0 healthy, 1 findings — and `--json` emits the same rows, so CI can gate on it.

In a scratch repository, seed a non-ignored file below the size limit and ask the agent to destroy it:

```console
$ echo "keep me" > smoke.txt
```

Tell the agent: *delete smoke.txt, then empty another file in this scratch repository.* The active hook should capture before each matching call. When it is done, inspect the retained steps and take them back:

```console
$ ff history
$ ff undo
```

[`ff history`](../reference/cli/history.md) shows adjacent captures from the same session collapsed into undo steps. Use as many steps as the map shows to reach the capture containing both original files, and verify their contents.

Nothing was discarded in the process. [`ff redo`](../reference/cli/redo.md) walks forward again if the agent's work turns out to be the version you wanted.

That round trip is the setup working, and it is the same round trip you will use on the day the break is not staged.
