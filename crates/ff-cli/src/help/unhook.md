Remove integrations installed by `ff hook`. With no slugs, report detected integrations and ask; name clients or shells to remove those, or use `--all` for all detected integrations.

## Examples

```sh
ff unhook                       # Report and ask
ff unhook claude                 # Remove one client integration
ff unhook --all                  # Remove all managed integrations
ff hook -l                      # Inspect the remaining setup
```

### Options

### What is removed

Fufu removes managed JSON commands, marked shell lines, the owned Claude Code, Codex, and Cursor plugin directories with the skill inside each, OpenCode's plugin file when fufu wrote it and its skill directory, the Copilot CLI plugin directory with its marketplace entry and settings registration, and fufu's entry in the Codex marketplace file. JSON commands matching current or retired fufu spellings are managed even when pasted by hand. Unrelated settings, other marketplace entries, and hand-written shell equivalents remain. Cursor's old managed MCP registration is removed; unrelated commands remain.

Restart the affected client or shell so its active configuration reflects the removal. Removing hooks does not remove repository history; fufu commands can still take snapshots when invoked.
