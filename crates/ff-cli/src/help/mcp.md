A Model Context Protocol server on stdin and stdout, for an agent client that wants fufu as a tool rather than as a shell command. `ff hook <client>` registers it with claude, codex, cursor, or gemini; this verb is what that registration runs.

It serves seven typed tools: `status`, `pull`, `push`, `undo`, `redo`, `explain`, and `help`. Each takes the verb's own flags as fields, generated from the same definitions `ff <verb> --help` reads, so a flag on the page is a field on the tool. These seven are the verbs where the shell adds nothing: the inputs are fixed and short, nothing about the output is something an agent would pipe, and the result's structure matters more than its text. Every other verb is the shell.

The server's `instructions` carry the same briefing the hook injects, so a client that surfaces instructions reads the doctrine there too.

```
{"name": "push", "arguments": {"dry-run": true}}
```

Every call runs this same binary as a child with `--json` and hands back the envelope, so capture, `fufu.gitPolicy`, sessions, error ids, and the no-prompt guarantee all hold. `help` is the exception: `{"verb": ["op", "log"]}` is `ff help op log` and returns the page as text, and with no `verb` it returns the map of every verb.

`isError` is true when an `error` envelope came back, and false on a `data` envelope whatever the exit code, so a `ff pull` that held is a successful call whose data says which branch held; the child's exit code rides every result as `_meta.exit`.

Two options change what a call does:

- `cwd` is a field on every tool, fufu's seven and a produced one alike, naming the directory to run in for a client that works across repositories. Without it the child runs where the client started the server.
- The session tags every operation the server's children record, which is how an agent's work stays separable from a person's, and it is settled with the same precedence every invocation has: `--session`, `FF_SESSION`, then the client's session (`CLAUDE_CODE_SESSION_ID` under Claude Code). All three are read once when the server starts: a client that changes its session without restarting the server, as Claude Code's `/clear` does, keeps the one it launched under.

### What is not served

Everything else is the shell, where the agent already has the whole surface and `ff help <verb>` for each piece of it. `ff extension` in particular stays a person's decision: the registry it writes is the allowlist for everything fufu says about an extension, so declaring is a decision about a machine and not one an agent makes for itself.

### Extensions

An extension whose manifest says `tools: true` gets the typed tools it produces listed beside the seven as `<extension>__<tool>`, each carrying hints of its own and taking `cwd` the way the seven do. The list is asked for once when the server starts and held for the life of the connection, so a restart is what picks up an edited extension.

A handshake that fails or hangs costs nothing and says nothing at the time. `ff doctor` is where it shows.

### Beside the capture hook

The two do different jobs, so wire both. The hook snapshots before every tool call the agent makes, whatever tool that is; the server only ever sees fufu verbs.

## Examples

```
ff mcp                       serve on stdio, until the client closes it
ff mcp --session flight-3    every child's operation carries the tag
ff hook claude               register the server with Claude Code
ff doctor                    the mcp row says which clients have it
```
