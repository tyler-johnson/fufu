Removes exactly what `ff hook` added, and nothing else. Foreign entries in a settings file keep whatever shape they had; a line somebody wrote by hand is left where it is and reported rather than removed.

The directories fufu owns outright — the Claude plugin, and the skill each client that reads one was given — go whole. A declared extension's own skill directory goes with them: nested inside the plugin for Claude, and by name beside fufu's own for Codex, removed for every extension still declared when `ff unhook` runs.

The MCP server's registration goes with the hook it was written beside, and one written by hand stays.

Bare `ff unhook` reports and asks, the same way `ff hook` does.

## Examples

```
ff unhook claude         one client
ff unhook --all          everything fufu wired
ff hook -l               what is wired, before and after
```
