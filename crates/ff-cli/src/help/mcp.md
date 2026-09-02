A Model Context Protocol server on stdin and stdout, for an agent client that wants fufu as a tool rather than as a shell command. It exposes one tool, `ff`, whose input is the command line after `ff` as an array of words: `{"args": ["commit", "-m", "parser: skeleton"]}`. Every call runs this same binary as a child with `--json` and hands back the envelope, so capture, `fufu.gitPolicy`, sessions, error ids, and the no-prompt guarantee all hold — the child is an ordinary invocation, and the server adds nothing to it.

One tool instead of one per verb, because the client transmits every tool's description on every turn: this one carries the verb list, the recovery table, and the landmines in about two thousand tokens, where forty typed tools would cost five times that and be a second spelling of the CLI to keep in step. The tool's description is assembled from the same sources as `ff --help` and the shipped skill, so it cannot drift from them.

Six verbs are not offered: `git`, `update`, `watch`, `hook`, `unhook`, and `mcp`. Each either owns its stream, talks a person through something, or wires the machine, and none of them makes sense inside a tool call. Asking for one returns `usage/mcp-verb-unavailable` and names a shell as the place to run it.

A call may carry `cwd`, the directory to run in, for a client that works across repositories. Without it, the child runs where the server was started, which is the directory the client launched it in. `--session <name>` on `ff mcp`, or `FF_SESSION` in its environment, tags every operation the server's children record, which is how an agent's work stays separable from a person's.

`ff hook <client>` registers the server with claude, codex, cursor, or gemini, alongside the capture hook it already wires; `ff unhook <client>` removes it, and `ff doctor` reports it. The two mechanisms do different jobs: the hook snapshots before every tool call the agent makes, whatever tool that is, while the server only sees fufu verbs. Wire both.

## Examples

```
ff mcp                       serve on stdio, until the client closes it
ff mcp --session flight-3    every child's operation carries the tag
ff hook claude               register the server with Claude Code
ff doctor                    the mcp row says which clients have it
```
