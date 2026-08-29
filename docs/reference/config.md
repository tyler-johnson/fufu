# Configuration

!!! note "Draft stub"
    Planned content, not yet written. Sources: help for `config`; the key registry in ff-cli.

To cover:

- `ff config` — reading, setting, and where values live (repo vs user).
- The key registry, one entry per key with default and effect: `fufu.gitPolicy`, `fufu.pager`, trunk detection, and the rest.
- Environment variables: `FF_PAGER`, and precedence against `PAGER` and `fufu.pager`.
- What fufu reads from *git's* config (user identity, `insteadOf`, proxies, credential helpers) versus what is fufu's own.
