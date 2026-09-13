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

Fufu removes its marked settings and shell entries, managed Claude plugin, and installed skills. Other settings and hand-written integration lines remain and are reported. Old fufu-managed MCP registrations are removed with their hooks; hand-written registrations remain.

Restart the affected client or shell so its active configuration reflects the removal. Removing hooks does not remove repository history; fufu commands can still take snapshots when invoked.
