# Hooks

[`ff hook <slug>`](../cli/hook.md) wires fufu into one shell or one agent client, and [`ff unhook <slug>`](../cli/unhook.md) takes back exactly what that added. The slugs are `bash`, `zsh`, `fish`, `claude`, `codex`, `cursor`, and `gemini`. One page per slug shows the files it writes, pasted from a run, and what unhook leaves behind.

Two mechanisms cover the seven. A shell takes marked lines in its rc file: the alias `git='ff git'`, so every git command you type snapshots first, and a prompt hook that runs [`ff trigger shell`](../cli/trigger.md) before each prompt. An agent client takes hook entries merged into a settings file it owns, each running `ff trigger <slug>` before a tool call and at the turn boundary, with the rest of the file left as it was. Claude Code is the exception: it takes a plugin directory fufu owns outright, written whole and removed whole.

The rules are the same everywhere:

- A line or an entry you wrote by hand is detected, reported, and never touched. In a shell the two pieces are independent, so a hand-written alias leaves the prompt hook to be installed and the other way around.
- A settings file that is not valid JSON is refused with the file untouched. fufu never rewrites a file into something the client cannot read.
- Running `ff hook <slug>` on a wired machine reports it as already wired and changes nothing, except to rewrite a spelling fufu no longer writes.
- `ff hook -l` reports the state of every slug and stops. [`ff doctor`](../doctor.md) reports the same state, one row per client plus rows for the alias, the prompt hook, and the skill, and `ff doctor --fix` rewires whatever is stale.

Nothing here reaches the network: every slug writes local files and nothing else.

- [bash](bash.md), [zsh](zsh.md), [fish](fish.md)
- [Claude Code](claude.md), [Codex](codex.md), [Cursor](cursor.md), [Gemini CLI](gemini.md)

[Setup for agents](../../agents/setup.md) is the guide side of this: what the hook does once wired, the briefing, and the hand-pasted two-event floor for Claude Code.
