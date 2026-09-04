Wires fufu into the agent clients and shells on this machine, so a snapshot lands before every tool call your agent makes, before every git command you type, and at every shell prompt. With none of them wired, fufu snapshots only when you type an ff command, and `ff doctor` warns that nothing is feeding capture.

Bare `ff hook` reports what it found and then asks. Name slugs to wire exactly those; --all takes everything detected without asking; -l reports and stops either way. The slugs are flat:

```
claude  codex  cursor  gemini  bash  zsh  fish  powershell
```

`cursor` is the agent client — a future editor integration gets its own name. `powershell` writes `$PROFILE`, a `git` function and a wrapped `prompt`, for PowerShell 7 or Windows PowerShell 5.1.

### What gets written

Not a choice you make. Claude Code takes a plugin directory fufu owns outright, the other three clients take entries merged into their own settings file, and the shells take marked lines in an rc file. A line you wrote yourself is detected, reported, and never touched.

Claude Code and Codex also take fufu's skill — the manual for what the once-per-session briefing has no room for, from recovery to rewriting commits that have closed. Claude's skill rides inside the plugin, so --settings wires capture and no skill.

### The MCP server

The four agent clients also get `ff mcp` registered as a server, so the agent can reach fufu as a typed tool:

```
claude   .mcp.json inside the plugin directory
codex    a marked block in config.toml
cursor   mcpServers.fufu in mcp.json
gemini   mcpServers.fufu in settings.json
```

The hook and the server do different jobs — the hook snapshots before every tool call, whatever the tool; the server only sees fufu verbs — so both are wired. A registration you wrote yourself is left alone.

### Declared extensions

An extension declared with `ff extension add` rides along in three ways:

- One line on the same briefing: the text its manifest carries, or whatever `ff-<name> briefing` prints when the briefing is built. An extension that is gone from PATH, broken, or slow contributes nothing and costs the briefing nothing.
- Its skill files, installed as `skills/<name>/` beside `skills/fufu/` for the same two clients. Cursor and Gemini read no skills directory, so an extension gets the briefing line there and nothing more. A file the manifest names but that is missing, unreadable, or too large to be a manual is left out rather than failing the install.
- Its own MCP server, when the manifest names one, registered beside fufu's as `mcpServers.<name>` and as a second table inside Codex's one block. A registration under its name that you wrote yourself is left alone the same way.

`ff hook --skill <name>` prints a declared extension's skill the way a bare `ff hook --skill` prints fufu's own. `ff unhook` takes fufu's wiring and the extensions' back together.

## Examples

```
ff hook                  what is on this machine, then asks
ff hook claude codex     wire exactly those
ff hook --all            everything detected, no question
ff hook -l               report and stop
ff hook --skill          print the manual, for a client that reads no skill
ff hook --skill tower    print a declared extension's own skill
ff unhook claude         take back exactly what hook added
ff doctor                check that something is feeding capture
```
