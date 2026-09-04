# ff hook

Wires fufu into the agent clients and shells on this machine, so a snapshot lands before every tool call your agent makes, before every git command you type, and at every shell prompt. That is the difference between "the agent broke something" and "the agent broke something, and here is the tree from thirty seconds ago".

Bare `ff hook` reports what it found and then asks. Name slugs to wire exactly those; --all takes everything detected without asking; -l reports and stops either way. The slugs are flat: claude, codex, cursor, gemini, bash, zsh, fish, powershell. `cursor` is the agent client — a future editor integration gets its own name. `powershell` writes `$PROFILE`, a `git` function and a wrapped `prompt`, for PowerShell 7 or Windows PowerShell 5.1.

What gets written depends on the client, and is not a choice you make: Claude Code takes a plugin directory fufu owns outright, the others take entries merged into their own settings file, and the shells take marked lines in an rc file. A line you wrote yourself is detected, reported, and never touched.

Claude Code and Codex take fufu's own skill along with the wiring: the manual for everything the once-per-session briefing has no room for — recovery, rewriting commits that have closed, held rewrites, the JSON. It costs the agent nothing until it is read. Claude's skill rides inside the plugin, so --settings wires capture and no skill.

An extension declared with [`ff extension add`](extension-add.md) adds one line to the same briefing: the text its manifest carries, or whatever `ff-<name> briefing` prints when the briefing is built, run in the event's own directory with the three variables an extension is handed anywhere else. The line rides the same boundaries fufu's own notice does and is capped at the same kind of budget, and an extension that is gone from PATH, broken, or slow contributes nothing and costs the briefing nothing.

A declared extension's own skill files ship the same way fufu's does, for the same two clients: `skills/<name>/` beside `skills/fufu/`, read from wherever the manifest named them. Cursor and Gemini read no skills directory, so an extension gets the briefing line there and nothing more. A file the manifest names but that is missing, unreadable, or too large to be a manual is left out rather than failing the install. `ff hook --skill <name>` prints a declared extension's skill the way a bare `ff hook --skill` prints fufu's own.

The four agent clients also get [`ff mcp`](mcp.md) registered as a server, so the agent can reach fufu as a typed tool: `.mcp.json` in the Claude plugin, a marked block in Codex's `config.toml`, `mcpServers.fufu` in Cursor's `mcp.json` and Gemini's `settings.json`. The hook and the server do different jobs — the hook snapshots before every tool call, whatever the tool; the server only sees fufu verbs — so both are wired. A registration you wrote yourself is left alone.

A declared extension whose manifest names a server of its own is registered beside fufu's in the same file, as `mcpServers.<name>` and as a second table inside Codex's one block, with whatever command, arguments and environment the manifest asked for. [`ff unhook`](unhook.md) takes them back together. That is where an extension wanting what only a live process can hold goes — resources, a notification when state moves, a subscription; typed tools alone are the manifest's `tools` field and need no server. A registration under its name that you wrote yourself is left alone the same way.

Hooks are what make capture ambient instead of something you remember. With none of them wired, fufu snapshots only when you type an ff command — which works, and misses the whole point. [`ff doctor`](doctor.md) warns when nothing at all feeds capture, because a silent engine feels safe while capturing nothing.

## Usage

```
Usage: ff hook [OPTIONS] [slug]...

Arguments:
  [slug]...
          Slugs to hook: claude, codex, cursor, gemini, bash, zsh, fish, powershell

Options:
      --all
          Everything detected, without asking

  -l, --list
          Report what is here and stop

      --settings
          claude only: wire settings entries instead of the plugin

      --json
          Emit machine-readable JSON

      --skill [<name>]
          Print a skill and stop: fufu's own with no name, a declared extension's with one

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

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
