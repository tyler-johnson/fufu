# Why agents want fufu

!!! note "Draft stub"
    Planned content, not yet written.

Version control for humans *and agents* is the pitch; this page is the argument. To cover:

- The failure mode: an agent with shell access and `git` is one confident `reset --hard` from destroying an afternoon — and agents run destructive commands with conviction.
- The net: fufu snapshots before every action, including foreign git commands, so the human can always `ff undo` whatever the agent did — tree and refs together.
- No staging means no half-staged states for an agent to mangle; no stash means nothing to forget; every verb reports what it did in one parseable line with `undo:` on the next.
- The supervisor pattern: agent works with `ff` (or even raw git), human reviews with `ff history` and `ff op diff`, disasters cost one keystroke.
- Strict mode as a leash: refusing the ambiguous git invocations outright.
