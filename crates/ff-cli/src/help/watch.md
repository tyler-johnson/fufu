The operation log, as a stream: one JSON object per line, written as the log moves. It is a foreground process you started rather than a daemon — it writes nothing and holds no authority. Ctrl-C ends it.

What arrives is what the log *did*, not what was appended to it:

- an operation landing is `landed`
- an undo moving the pointer back is `stepped-back`
- work after an undo is `forked`
- a trim is `rewritten`, and that one is terminal: every operation id you were holding stops resolving there, so the stream ends and exit 1 says so

Every stream opens on `start`, which names the tip you begin from.

Operations are written before the mutation they describe, so an event is a claim about the next microsecond rather than a report on the last one. `ff op log` shows the same operation with the same caveat.

### Every worktree

--all streams the whole repository instead of this worktree. It opens with one `start` per worktree, picks up a worktree added while it runs, and keeps a removed worktree's chain on the stream, since the removal captures into that chain and dropping it would lose the last thing it said.

Every line carries the worktree the operation belongs to, in both modes, so a merged stream is one shape to parse.

A rewrite under --all belongs to the chain it happened to: that chain is reported, re-anchors itself, and the other chains keep streaming, so there is no exit 1.

--since replays from an operation before tailing, and is refused with --all: an operation belongs to one chain, and there is no single place a repository-wide stream would replay from.

## Examples

```
ff watch                      this worktree, until you stop it
ff watch --all                every worktree in the repository
ff watch --kind op            verbs only, no captures
ff watch --session flight-3   one agent's motion
ff watch -n 1                 the current tip, then exit
ff op log                     the same operations, as a page
```
