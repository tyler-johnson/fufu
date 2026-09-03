# Agent setup

[Why agents want fufu](why.md) is the argument; this page is the wiring. Six steps: pick a git policy, put standing orders where the agent reads them, wire the per-turn hook, ship the skill, serve the verbs as a tool, and verify the whole net — including breaking something on purpose to watch [`ff undo`](../reference/cli/undo.md) take it back. `ff hook --all` does the middle four in one command; the blocks below are for reading what it wires, and for pasting by hand where an installer cannot reach.

## Pick a git policy

`fufu.gitPolicy` decides what fufu says when git is reached for directly — through the [`ff git`](../reference/cli/git.md) passthrough, or in the agent's own shell through the hook. Three levels:

- **`observe`** records the git word and says nothing. Capture still runs first; you get the tally without the commentary.
- **`coach`** — the default — injects one line naming the fufu verb the first time each git word comes up: `tip: that's ff commit`. An agent reads the correction as an instruction, so this is usually enough.
- **`strict`** refuses a git write that has a fufu verb and names what to run instead. Through the hook the refusal travels as JSON (`permissionDecision: deny`); on the alias it is exit 2.

Set it per repository with [`ff config`](../reference/cli/config.md), which validates the value before writing it:

```console
$ ff config gitPolicy strict
```

For an agent you are watching, `coach` is the right default — one correction is cheap, and the agent adjusts. For an unattended or long-running agent, set `strict`; the fufu repository itself runs under it. Know what strict does not promise before you lean on it: git words with no fufu verb pass untouched, ambiguous shell strings fail open, and `ff git <args…>` stays an open escape hatch — [why.md walks the limits](why.md#strict-mode-as-a-leash). Under every level the snapshot lands before the command runs, so the policy call never decides whether the tree is recoverable.

## Standing orders: the CLAUDE.md / AGENTS.md block

The agent needs one paragraph of doctrine: write through `ff`, and never write a backup copy. Paste this into your project's `CLAUDE.md`, `AGENTS.md`, or whatever memory file your client reads — it is the same text fufu's own briefing carries:

```markdown
## Version control

fufu (`ff`) is capturing this repository: the worktree is snapshotted before every tool action, so no edit can lose file state. Work directly — no backup copies, no hedging.

Use `ff`, not `git`, for anything that writes. `ff commit -m "…"` closes the open change — no add, no staging, the worktree is the change. `ff switch <branch>` moves. `ff undo` takes back the last operation. `ff restore <path>` discards a file's edits. Anything else git does: `ff git <args…>`, which snapshots and then runs git verbatim.

Reading with git is fine. `ff status`, `ff log`, and `ff diff` say more than their git counterparts.

When an `ff` tool is offered, call it with the same words instead of the shell.

Every verb's own `--help` is the authority on it.
```

With the hook below wired, fufu injects this briefing itself — at the turn boundary, again after anything that rebuilds the context (a resume, a `/clear`, a compaction), and once for each subagent. The file copy is for clients without a hook channel, and for repositories where you want the doctrine standing in project memory regardless of which machine the agent runs on. Having both costs a few lines.

## The per-turn hook

The hook is what makes capture ambient: a snapshot before every tool call the agent makes, so the last capture is always the moment before the agent's action. One command wires it:

```console
$ ff hook claude
```

The slugs are `claude`, `codex`, `cursor`, `gemini`, plus `bash`, `zsh`, `fish`, and `powershell` for the shell alias and prompt hook; bare [`ff hook`](../reference/cli/hook.md) reports what it detects and asks, and `ff hook --all` takes everything without asking. For Claude Code the installer writes a plugin directory fufu owns outright; for the other clients it merges entries into their own settings file, and it never touches a line you wrote yourself. [The hook reference](../reference/hooks/index.md) shows the files each slug writes and what [`ff unhook`](../reference/cli/unhook.md) removes.

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

These two events are the floor. `PreToolUse` is the capture that cannot miss — every edit, every shell command, and the only channel that reaches a subagent or a repository the agent just entered. `UserPromptSubmit` is the turn boundary the briefing rides. The installer wires five more (`SessionStart`, `Stop`, `SubagentStop`, `SubagentStart`, `CwdChanged`) that widen capture rather than found it — the `Stop` pair matters most, because capture is snapshot-*before*, and without a turn-end event the file state an agent writes as its final action sits uncaptured until whatever comes next.

### The same wiring for Codex

`ff hook codex` wires the same floor into Codex: it merges two events into `~/.codex/hooks.json` — `PreToolUse` on the tools that mutate, and the turn boundary — and writes [the skill](#ship-the-skill) to `~/.codex/skills/fufu/`. This is what lands:

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

Two honest differences from the Claude Code wiring. fufu wires Codex with the floor and nothing wider — the turn-end and subagent events the Claude Code installer adds have no counterpart here today, so the file state an agent writes as its final action waits for the next turn's capture. And Codex trusts hooks by their hash: after `ff hook codex`, run `/hooks` inside Codex once to review and accept the new hook, or Codex skips it and nothing captures. `ff doctor` keeps showing that reminder whenever the wiring is present, because fufu cannot read Codex's trust list — an unreviewed hook and a reviewed one look identical from outside.

[`ff trigger <source>`](../reference/cli/trigger.md) is built for this seat: it reads the client's payload on stdin, always exits 0 whatever went wrong, and never vetoes a tool call on its own judgment — the two vetoes there are, `fufu.gitPolicy strict` for raw git and `fufu.toolPolicy strict` for `ff` in the shell while the `ff` tool is up, are config saying so, and each travels as JSON the client enforces. A source name fufu does not know exits 0 silently, so the same command is safe to wire into a client fufu has never heard of. When your agent is a script rather than a client with hooks, [the machine surface](machine-surface.md) is the contract to build against.

Two agents in one repository is a supported shape, and the wiring above is all it takes. Give each agent its own worktree — [`ff worktree add`](../reference/cli/worktree-add.md) — and their operation logs never touch: each worktree writes its own chain under its own lock, so parallel agents cannot contend, and `ff undo` in one tree steps back only that tree's work. Two agents sharing a single worktree settle at that chain's lock instead: a capture that loses it is skipped, because the winner is already recording, and a verb waits briefly and then refuses with `ref/contended`, exit 4, rather than interleaving; run it again. One chain is also one undo: `ff undo` takes back the last operation whoever wrote it, so undoing agent A's mistake after agent B has moved on takes back B's work first. The scoped verb is [`ff op revert <op>`](../reference/cli/op-revert.md), which inverts one operation and leaves later ones standing, where the refs it moved have not moved since; [two writers on one chain](../guides/recovery.md#two-writers-on-one-chain-and-only-one-was-wrong) in the recovery cookbook walks it. The [worktrees guide](../guides/worktrees.md#two-writers-one-repository) has the full story, including watching every tree's motion from one seat.

## Ship the skill

The briefing is deliberately short — four verbs, the git rule, a pointer to `--help` — because the agent pays for it every session. Everything past that lives in a skill fufu ships: the recovery table, rewriting commits that have already closed, held rewrites and conflicts, the landmines, and the JSON surface. It costs the agent nothing until the situation calls for it, and it is the difference between an agent that reads [`ff evolog`](../reference/cli/evolog.md) to find a lost hour and one that improvises reflog archaeology through `ff git`.

`ff hook claude` and `ff hook codex` install the skill beside the wiring — [the Codex subsection above](#the-same-wiring-for-codex) says where — so there is nothing extra to do on those clients. For a client that reads no skills directory, print it and put it wherever your agent reads instructions:

```console
$ ff hook --skill
```

## Serve the verbs as a tool

The hook makes fufu ambient; [`ff mcp`](../reference/cli/mcp.md) makes it a tool the agent can reach for by name. It is a Model Context Protocol server on stdio exposing one tool, `ff`, whose input is the command line after `ff` as an array — `{"args": ["commit", "-m", "parser: skeleton"]}` — and whose result is fufu's JSON envelope, as text and as structured content, with `isError` following the exit code. Every call runs the binary as a child with `--json`, so nothing changes underneath: the child captures first, `fufu.gitPolicy` applies, `held/*` still means nothing moved and a person is needed, and no call can block on a prompt. What the agent gains is a typed, allowlistable tool with structured results in place of a shell string it has to quote, and a tool description that names every verb with a digest of recovery and the landmines in under two thousand characters, which is what a client shows the model of a description and why there is one tool rather than one per verb. The briefing and the skill both say to prefer the tool when it is offered; the words are the same either way.

Prose does not get to 100%, so `fufu.toolPolicy` backs it with the same channel `gitPolicy` uses. While the server is up for a client, an `ff` the agent runs in its shell tool is, under **`strict`** (the default), refused with a reason naming the tool and the exact `args` to call it with; under **`coach`** it is allowed and the tool is named once per session as context; under **`observe`** nothing is said. The six shell-only verbs pass under every tier, a path to a binary or a `sudo` in front of `ff` is not `ff`, and a compound command is refused by its `ff` segment, since `cd sub && ff status` is exactly what the tool's `cwd` is for. Presence is what makes this safe to default on: the server holds an exclusive lock on a marker under the user cache directory, named by the pid of the client that spawned it, for as long as it serves. The hook is handed that pid by Claude Code, tries a shared lock, and a refused lock is the only thing that counts as "up" — the OS releases the lock on any exit, so a killed server refuses nothing, and a marker nobody holds is swept by the first hook that reads it or the next server that starts. Only Claude Code sets the pid and only Claude Code has a deny channel, so the other clients are never refused. `ff config toolPolicy coach` moves it.

What it does not replace is the hook. The server sees only fufu verbs; the snapshot before every *other* tool call — an edit, a shell command — still rides `PreToolUse`, so wire both. Six verbs are not offered through the tool, because each owns its stream or wires the machine: `git`, `update`, `watch`, `hook`, `unhook`, and `mcp`. Asking for one returns `usage/mcp-verb-unavailable`. `--session` on the server, or `FF_SESSION` in its environment, tags every child's operations, so an agent's work through the tool is separable in [`ff op log`](../reference/cli/op-log.md) the same way its hook captures are.

`ff hook <client>` registers the server beside the hook it wires, and `ff unhook <client>` removes it. Where each client keeps it, and the name the tool takes there:

| client | file | tool |
| --- | --- | --- |
| Claude Code | `.mcp.json` in the plugin at `~/.claude/skills/fufu/` | `mcp__plugin_fufu_fufu__ff` |
| Codex | a marked `[mcp_servers.fufu]` block in `~/.codex/config.toml` | `fufu`'s `ff` |
| Cursor | `mcpServers.fufu` in `~/.cursor/mcp.json` | `fufu`'s `ff` |
| Gemini CLI | `mcpServers.fufu` in `~/.gemini/settings.json` | `fufu`'s `ff` |

For a client that registers servers from a file you manage yourself, the entry is one key:

```json
{
  "mcpServers": {
    "fufu": {
      "type": "stdio",
      "command": "/usr/local/bin/ff",
      "args": ["mcp"]
    }
  }
}
```

`command` is the absolute path of your `ff`; the installer bakes it in so the server does not depend on the client's `PATH`. In Claude Code that entry goes in `~/.claude.json` at user scope, which is also what `claude mcp add --scope user fufu -- ff mcp` writes, and the tool is then `mcp__fufu__ff`. Codex takes the same thing as TOML:

```toml
[mcp_servers.fufu]
command = "/usr/local/bin/ff"
args = ["mcp"]
```

A registration you wrote by hand is detected and left alone by both `ff hook` and `ff unhook`. [The hook reference](../reference/hooks/index.md) shows each file as the installer leaves it.

## Verify

[`ff doctor`](../reference/cli/doctor.md) reads the whole net in one pass, and its wiring lane is the part this page set up. Healthy rows name where each hook landed:

```console
$ ff doctor
  ok    log            refs/fufu/wt/main/ops, newest operation 2m ago
  ...
  info  settings       gitPolicy strict
  ok    claude         plugin wired in ~/.claude/skills/fufu
  ok    skill          fufu's manual, for claude
  ok    mcp            registered with claude
  ok    alias          git='ff git' wired in ~/.bashrc (`ff hook bash` manages it)
  ok    ambient        prompt hook snapshots at every prompt, wired in ~/.bashrc (`ff hook bash` manages it)
```

A half-wired client is a `WARN` naming the missing event, and `ff hook <slug>` repairs it; so is a client whose hook is wired without the server, the shape an install from before `ff mcp` leaves, and `ff doctor --fix` runs the installer again. When nothing at all feeds capture, doctor warns about that too, because a silent engine feels safe while capturing nothing. Findings drive the exit code — 0 healthy, 1 findings — and `--json` emits the same rows, so CI can gate on it.

Then run the one test that exercises the actual promise. Seed a file, and ask the agent to destroy it:

```console
$ echo "keep me" > smoke.txt
```

Tell the agent: *delete smoke.txt, then empty another file in this repository.* Whatever route it takes — its editing tools, raw git, `ff git` — the hook captures the tree before each call. When it is done, look at the session and take it back:

```console
$ ff history
$ ff undo
```

[`ff history`](../reference/cli/history.md) shows the agent's machine-rate captures collapsed into the rows they undo as, and one `ff undo` returns the tree — `smoke.txt` back on disk, the emptied file whole. Nothing was discarded in the process: [`ff redo`](../reference/cli/redo.md) walks forward again if the agent's work turns out to be the version you wanted. That round trip is the setup working, and it is the same round trip you will use on the day the break is not staged.
