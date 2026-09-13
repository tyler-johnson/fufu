# Agent setup

Install the hook for the client you use, activate it, then verify a captured edit in a scratch repository. You need [fufu installed](../install.md) and available to the client's shell.

## Install and activate

Run the matching [`ff hook`](../reference/cli/hook.md) command in your terminal:

| Client | Install | Required next step |
| --- | --- | --- |
| [Claude Code](../reference/hooks/claude.md) | `ff hook claude` | **Restart Claude Code** to load the plugin. `claude plugin list` should show `fufu@skills-dir`. |
| [Codex](../reference/hooks/codex.md) | `ff hook codex` | **Run `/hooks` inside Codex and review and accept the hook.** New or changed hooks are skipped until approved by hash. |
| [Cursor agent](../reference/hooks/cursor.md) | `ff hook cursor` | Start a new agent session to check the loaded configuration. Cloud agents lack the session-start briefing; use project instructions below. |
| [Gemini CLI](../reference/hooks/gemini.md) | `ff hook gemini` | Start a new CLI session to check the loaded configuration. |

Claude Code and Codex installations include the shipped skill. Cursor and Gemini installations provide hooks but do not install a skill. The linked client references describe managed files, removal, and migration. Integrations are installed per machine; repeat setup on each machine where the agent runs.

`ff hook --all` installs integrations for detected clients and shells without asking. Follow each reported activation step. It does not add instructions to your project's `CLAUDE.md` or `AGENTS.md`. Shell hooks need their own activation; see the [shell references](../reference/hooks/index.md).

## Verify

First check configuration with `ff hook -l` and [`ff doctor`](../reference/cli/doctor.md) in a repository. Doctor exits 0 when healthy and 1 on findings; read the named checks. Re-run `ff hook <client>` to repair partial or stale wiring, then repeat activation. `ff doctor --fix` also repairs supported hook and skill findings.

An installed configuration does **not** prove that a running client loaded or trusted it. Doctor cannot read Codex's trust list, so its `/hooks` reminder remains after approval. Doctor itself attempts capture and reconciliation, can fetch when enabled, and can run maintenance; it is not a passive hook-activation probe.

For an end-to-end check, create an isolated repository with [`ff init`](../reference/cli/init.md). These shell commands use a temporary directory and a small, non-ignored file:

```sh
scratch=$(mktemp -d)
ff init "$scratch"
cd "$scratch"
printf 'keep me\n' > smoke.txt
```

Open your activated client in that directory. Ask it: **“Delete smoke.txt using one file-edit or shell tool call.”** Do not run a manual snapshot first: this check needs evidence from the client hook. After deletion, inspect [`ff op log`](../reference/cli/op-log.md):

```sh
ff op log
```

Find the capture immediately before the deletion, with the client's name and tool in its summary. Copy its hexadecimal operation ID. [`ff op show`](../reference/cli/op-show.md) inspects the record; [`ff restore`](../reference/cli/restore.md) recovers just the file. Replace `<capture-id>` below with the ID you found:

```sh
ff op show <capture-id>
ff restore smoke.txt --at-op <capture-id>
cat smoke.txt
```

The file should contain `keep me`. If no pre-deletion capture exists, recheck activation, Codex approval, the tool matcher, and the client's hook logs. A later reader can capture the deleted state, but cannot reconstruct bytes the hook never recorded. This test verifies the event you exercised; [coverage and limits](../concepts/snapshots-and-undo.md#coverage-and-limits) still apply.

For whole-state recovery, [`ff history`](../reference/cli/history.md) shows the undo steps and [`ff undo`](../reference/cli/undo.md) moves through them. Consecutive captures can collapse into one step, so an undo may go farther back than a particular edit. Use a capture ID for precise file recovery; see [the recovery guide](../guides/recovery.md#restore-one-file).

<a id="the-per-turn-hook"></a>
## What the hook captures and what it tells the agent

Snapshots and briefings serve different purposes. Tool events attempt a snapshot **before** the matching action. Turn or session events can also capture; when the client has a context channel, they deliver short instructions. The briefing is deduplicated per repository, session, and supported audience rather than repeated on every tool call.

| Client | Before-tool capture matcher | Other installed events and briefing coverage |
| --- | --- | --- |
| Claude Code | `Bash`, `Edit`, `Write`, `NotebookEdit` | `UserPromptSubmit` briefs once; `SessionStart` rebriefs on startup, resume, clear, compact, or fork. `Stop`, `SubagentStop`, `SubagentStart`, and `CwdChanged` widen capture. Pre-tool replies can brief subagents and newly entered repositories. |
| Codex | `Bash`, `apply_patch` | `UserPromptSubmit` captures and briefs. No installed turn-end or subagent events; pre-tool replies are silent. |
| Cursor agent | `Shell`, `Write`, `Delete` | `sessionStart` captures and briefs. It does not fire for cloud agents; pre-tool capture still runs when received, but supplies no briefing. |
| Gemini CLI | `run_shell_command`, `write_file`, `replace` | `SessionStart` captures and briefs. Pre-tool replies are silent. |

Claude Code's stop events can capture the final edit of a turn. The other installers have no turn-end capture event, so a final edit waits for the next received event or repository command. Every hook capture is best-effort: unchanged content adds no capture, and contention or failure can skip one. A skipped capture is not proof that another writer saved those same bytes.

[`ff trigger <client>`](../reference/cli/trigger.md) reads client payloads on stdin. Runtime failures exit 0 quietly unless `FF_DEBUG` is set; successful replies use the client's protocol. Scripts should use the [JSON output and scripting](machine-surface.md) interface instead.

## Pick a git policy

`fufu.gitPolicy` controls recognized Git writes through [`ff git`](../reference/cli/git.md) and active agent hooks:

| Policy | Effect |
| --- | --- |
| `observe` | Record recognized Git writes without advice. |
| `coach` (default) | Suggest the matching fufu command, once per Git word and session where a reply channel exists. |
| `strict` | Refuse mapped writes through the passthrough; request denial through a client adapter that supports it. |

Set a repository override with [`ff config`](../reference/cli/config.md):

```sh
ff config gitPolicy strict
```

**Only the current Claude Code adapter emits pre-tool coaching and denial replies.** Codex, Cursor, and Gemini adapters capture and record the policy tally but emit no tool reply, including under `strict`. The Claude Code denial also depends on the client enforcing its response. Use `ff git` for passthrough policy enforcement in any client.

A strict passthrough refusal exits 2 before capture or Git execution, while still recording the policy tally. An allowed passthrough attempts capture and runs Git even if that capture fails with a warning. Agent hooks attempt capture before evaluating policy. Unmapped Git commands and ambiguous shell strings are allowed; [Using fufu alongside Git](../concepts/two-regimes.md#which-program-ran) describes aliases and the policy's limits.

<a id="standing-orders-the-claudemd-agentsmd-block"></a>
## Optional project instructions

For clients without a briefing channel, or repository-specific standing instructions, add a short block to the memory file your client reads. The installer does not write it. This is a project-ready summary; the hook's built-in briefing is maintained separately.

```markdown
## Version control

Use fufu (`ff`) for version-control writes. `ff commit -m "…"` records eligible working-copy changes without staging. `ff switch <branch>` parks current work and resumes the destination's work. Reading with Git is fine; use `ff git <args…>` for permitted passthrough commands.

Recovery needs a successful retained snapshot. Inspect `ff history` for undo steps or `ff op log` for capture IDs before choosing `ff undo` or `ff restore <path> --at-op <id>`. Read the fufu skill for advanced work; `ff hook --skill` prints it. Every verb's `--help` is the authority.
```

## Ship the skill

The briefing covers everyday commands and routes advanced tasks to the skill. The skill contains the recovery decision table, rewriting and conflict workflows, ID distinctions, and scripting caveats.

`ff hook claude` installs it inside the plugin; `ff hook codex` writes `~/.codex/skills/fufu/SKILL.md`. Re-running the installer refreshes the shipped copy. For another client or hand-managed instructions, print it and place it where that client reads instructions:

```sh
ff hook --skill
```

## Manual hook configuration

Prefer the installers, which maintain the current event set. For hand-managed Claude Code settings, `ff hook claude --settings` merges all seven events instead of installing a plugin, and carries no skill. The [Claude Code reference](../reference/hooks/claude.md#the-settings-escape-hatch) describes that choice.

This minimal `.claude/settings.json` example installs only before-tool and turn-boundary capture and briefing. It omits the five wider events, including turn-end capture, and the skill:

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash|Edit|Write|NotebookEdit",
      "hooks": [{"type": "command", "command": "ff trigger claude"}]
    }],
    "UserPromptSubmit": [{
      "hooks": [{"type": "command", "command": "ff trigger claude"}]
    }]
  }
}
```

<a id="the-same-wiring-for-codex"></a>
Codex's exact JSON and trust instructions live in the [Codex reference](../reference/hooks/codex.md). When scripting a Claude Code installation, close stdin with `< /dev/null`: the legacy hook spelling first checks piped input for a trigger payload.

<a id="two-agents-in-one-repository"></a>
## Multiple agents

Give independent writers separate checkouts with [`ff worktree`](../reference/cli/worktree.md). Each has its own operation chain, capture lock, and undo history; shared refs can still contend, and branches checked out elsewhere have guards.

Agents sharing one worktree share files and undo steps. Session tags help identify records but do not isolate edits. A contended verb exits 4 with `ref/contended`; inspect its result and retry with a bounded policy. A hook that loses the lock skips capture.

[`ff op revert <op>`](../reference/cli/op-revert.md) can reverse an applicable operation's ref transitions only while those refs still equal the values it left. It preserves files, index, and HEAD selection; it cannot selectively erase one agent's file edits. Use the [two-writer recovery recipe](../guides/recovery.md#two-writers-on-one-chain-and-only-one-was-wrong) or [worktrees guide](../guides/worktrees.md#two-writers-one-repository) to choose the recovery scope.
