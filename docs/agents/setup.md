# Agent setup

[Why agents want fufu](why.md) is the argument; this page is the wiring.

Six steps: pick a git policy, put standing orders where the agent reads them, wire the per-turn hook, ship the skill, serve the verbs as a tool, and verify the net. The last step includes breaking something on purpose to watch [`ff undo`](../reference/cli/undo.md) take it back.

`ff hook --all` does the middle four in one command. The blocks below are for reading what it wires, and for pasting by hand where an installer cannot reach.

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

Under every level the snapshot lands before the command runs, so the policy call never decides whether the tree is recoverable.

## Standing orders: the CLAUDE.md / AGENTS.md block

The agent needs one paragraph of doctrine: write through `ff`, and never write a backup copy. Paste this into your project's `CLAUDE.md`, `AGENTS.md`, or whatever memory file your client reads. It is the same text fufu's own briefing carries:

```markdown
## Version control

fufu (`ff`) is capturing this repository: the working copy is snapshotted before every tool action, so no edit can lose file state. Work directly — no backup copies, no hedging.

Use `ff`, not `git`, for anything that writes. `ff commit -m "…"` closes the open change — no add, no staging, the working copy is the change. `ff switch <branch>` moves. `ff undo` takes back the last operation. `ff restore <path>` discards a file's edits. Anything else git does: `ff git <args…>`, which snapshots and then runs git verbatim.

Reading with git is fine. `ff status`, `ff log`, and `ff diff` say more than their git counterparts.

Every verb's own `--help` is the authority on it.
```

With the hook below wired, fufu injects this briefing itself: at the turn boundary, again after anything that rebuilds the context (a resume, a `/clear`, a compaction), and once for each subagent.

The file copy is for clients without a hook channel, and for repositories where you want the doctrine standing in project memory regardless of which machine the agent runs on. Having both costs a few lines.

## The per-turn hook

The hook is what makes capture ambient: a snapshot before every tool call the agent makes, so the last capture is always the moment before the agent's action. One command wires it:

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

These two events are the floor. `PreToolUse` is the capture that cannot miss — every edit, every shell command, and the only channel that reaches a subagent or a repository the agent just entered. `UserPromptSubmit` is the turn boundary the briefing rides.

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

- **A worktree each.** Give every agent its own with [`ff worktree <path>`](../reference/cli/worktree.md), and their operation logs never touch. Each worktree writes its own chain — its own line of captures and operations — under its own lock, so parallel agents cannot contend, and `ff undo` in one tree steps back only that tree's work.
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

### An extension's briefing line

An extension declared with [`ff extension <name>`](../reference/cli/extension.md) may add one line to the same briefing, and one line is the whole of what it may add.

The manifest's `briefing` field says where the line comes from:

- a string there is the line itself.
- `true` there means fufu runs `ff-<name> briefing` when the briefing is built, in the event's own directory, and takes its stdout.

Either way the line is trimmed to one line and capped at 240 characters. A line past the cap is dropped rather than cut, because half a sentence is still prose the agent reads as instructions.

The lines ride the same boundaries fufu's own notice does, and come out in the order the extensions were declared, after everything fufu had to say.

Failing to produce one costs nothing. A binary that has left PATH, one that will not start, one that exits nonzero, one that prints something that is not a line, and one still thinking when the time box runs out are all the same outcome: no line, no message to the agent, and a briefing exactly as it would have been.

That is `ff trigger`'s doctrine applied to the one place fufu invites an extension to speak into an agent's context. `FF_DEBUG=1` is where the reason goes when you want one. [Extensions](../reference/extensions.md) is the reference for building one.

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

Then run the one test that exercises the actual promise. Seed a file, and ask the agent to destroy it:

```console
$ echo "keep me" > smoke.txt
```

Tell the agent: *delete smoke.txt, then empty another file in this repository.* Whatever route it takes — its editing tools, raw git, `ff git` — the hook captures the tree before each call. When it is done, look at the session and take it back:

```console
$ ff history
$ ff undo
```

[`ff history`](../reference/cli/history.md) shows the agent's machine-rate captures collapsed into the rows they undo as, and one `ff undo` returns the tree: `smoke.txt` back on disk, the emptied file whole.

Nothing was discarded in the process. [`ff redo`](../reference/cli/redo.md) walks forward again if the agent's work turns out to be the version you wanted.

That round trip is the setup working, and it is the same round trip you will use on the day the break is not staged.
