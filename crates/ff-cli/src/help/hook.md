Wires fufu into the agent clients and shells on this machine, so a snapshot lands before every tool call your agent makes, before every git command you type, and at every shell prompt. That is the difference between "the agent broke something" and "the agent broke something, and here is the tree from thirty seconds ago".

Bare `ff hook` reports what it found and then asks. Name slugs to wire exactly those; --all takes everything detected without asking; -l reports and stops either way. The slugs are flat: claude, codex, cursor, gemini, bash, zsh, fish, powershell. `cursor` is the agent client — a future editor integration gets its own name. `powershell` writes `$PROFILE`, a `git` function and a wrapped `prompt`, for PowerShell 7 or Windows PowerShell 5.1.

What gets written depends on the client, and is not a choice you make: Claude Code takes a plugin directory fufu owns outright, the others take entries merged into their own settings file, and the shells take marked lines in an rc file. A line you wrote yourself is detected, reported, and never touched.

Claude Code and Codex take fufu's own skill along with the wiring: the manual for everything the once-per-session briefing has no room for — recovery, rewriting commits that have closed, held rewrites, the JSON. It costs the agent nothing until it is read. Claude's skill rides inside the plugin, so --settings wires capture and no skill.

The four agent clients also get `ff mcp` registered as a server, so the agent can reach fufu as a typed tool: `.mcp.json` in the Claude plugin, a marked block in Codex's `config.toml`, `mcpServers.fufu` in Cursor's `mcp.json` and Gemini's `settings.json`. The hook and the server do different jobs — the hook snapshots before every tool call, whatever the tool; the server only sees fufu verbs — so both are wired. A registration you wrote yourself is left alone.

Hooks are what make capture ambient instead of something you remember. With none of them wired, fufu snapshots only when you type an ff command — which works, and misses the whole point. `ff doctor` warns when nothing at all feeds capture, because a silent engine feels safe while capturing nothing.

## Examples

```
ff hook                  what is on this machine, then asks
ff hook claude codex     wire exactly those
ff hook --all            everything detected, no question
ff hook -l               report and stop
ff hook --skill          print the manual, for a client that reads no skill
ff unhook claude         take back exactly what hook added
ff doctor                check that something is feeding capture
```
