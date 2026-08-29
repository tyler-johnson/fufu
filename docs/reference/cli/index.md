# CLI reference

!!! note "Draft stub"
    This section should be **generated, not written**: the help prose already lives in `crates/ff-cli/src/help/*.txt`, one file per verb, and a small build step turns each into a page here so the reference can never drift from `--help`. Grouping mirrors the root help page.

Until the generator exists, `ff help <verb>` is the reference. The surface, grouped as the root page groups it:

- **Daily loop** — `start`, `describe`, `commit`, `switch`, `sync`, `publish`
- **Reading** — `ff` (the map), `status`, `log`, `diff`, `show`, `history`, `evolog`, `root`, `version`
- **Undo** — `undo`, `redo`, `op` (`log`, `show`, `diff`, `restore`, `revert`)
- **Rewriting** — `absorb`, `edit`, `done`, `lift`, `collide`, `restack`, `resolve`, `restore`, `trim`
- **Repository** — `init`, `clone`, `branch`, `remote`, `worktree`, `watch`, `hook`, `unhook`, `trigger`
- **Escape and upkeep** — `git`, `config`, `doctor`, `update`
