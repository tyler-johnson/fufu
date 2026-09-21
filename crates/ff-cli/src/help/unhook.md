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

Fufu removes the selected integration's managed hooks, plugin files, skills, and registrations. Shared settings and marketplaces retain unrelated entries. OpenCode's `fufu.js` is removed only when it has fufu's ownership header.

JSON commands matching current or retired fufu spellings are managed even when pasted by hand. Hand-written shell equivalents remain. The [client references](../hooks/index.md) list the files and legacy registrations each uninstaller removes.

Restart the affected client or shell so its active configuration reflects the removal. Removing hooks does not remove repository history; fufu commands can still take snapshots when invoked.
