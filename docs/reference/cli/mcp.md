# ff mcp

A Model Context Protocol server on stdin and stdout, for an agent client that wants fufu as a tool rather than as a shell command. [`ff hook <client>`](hook.md) registers it with claude, codex, cursor, or gemini; this verb is what that registration runs.

It exposes one tool, `ff`, whose input is the command line after `ff` as an array of words:

```
{"args": ["commit", "-m", "parser: skeleton"]}
```

Every call runs this same binary as a child with `--json` and hands back the envelope, so capture, `fufu.gitPolicy`, sessions, error ids, and the no-prompt guarantee all hold. There is one tool rather than one per verb because a client shows the model only the first two thousand characters of each description, and forty of them would be forty cards.

Two options change what a call does:

- `cwd` on the call names the directory to run in, for a client that works across repositories. Without it the child runs where the client started the server.
- `--session <name>` here, or `FF_SESSION` in the environment, tags every operation the server's children record, which is how an agent's work stays separable from a person's.

## What is not served

```
git  update  watch  hook  unhook  mcp  extension
```

Each owns its stream, talks a person through something, or wires the machine. Asking for one returns `usage/mcp-verb-unavailable` and names a shell as the place to run it.

`extension` is the one that is more than a bad fit: the registry it writes is the allowlist for everything fufu says about an extension, so declaring stays a person's decision about a machine.

## Extensions

A declared extension is relayed the way a verb is. [`ff extension add <name>`](extension-add.md) records its manifest, and from then on the child dispatches to `ff-<name>` and hands back the envelope it printed. Two refusals stand in the way:

- `usage/mcp-extension-undeclared` — nobody declared it. The exit names `ff extension add <name>`, and a shell is where it runs until then.
- `usage/mcp-extension-not-undoable` — its manifest says `undoable: false`. This tool's annotations promise that nothing it relays is destructive, which is honest only of an extension whose writes [`ff undo`](undo.md) takes back.

That second refusal costs the args array and nothing else. An extension whose manifest says `tools: true` gets the typed tools it produces listed beside `ff` as `<extension>__<tool>`, undoable or not, because a produced tool carries hints of its own. The list is asked for once when the server starts and held for the life of the connection, so a restart is what picks up an edited extension.

A handshake that fails or hangs costs nothing and says nothing at the time. [`ff doctor`](doctor.md) is where it shows.

## Beside the capture hook

The two do different jobs, so wire both. The hook snapshots before every tool call the agent makes, whatever tool that is; the server only ever sees fufu verbs.

They also talk. While it serves, the server holds a presence marker under the user cache directory, keyed by the client that spawned it. That marker is what lets the hook refuse `ff` in the shell under `fufu.toolPolicy` only while the tool is actually up — one nobody holds counts for nothing.

Only fufu's own marker is read there. A declared extension's own MCP server is a process the client starts and fufu never sees, so a registration on disk says it is installed and nothing says it is running.

## Usage

```
Usage: ff mcp [OPTIONS]

Options:
      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff mcp                       serve on stdio, until the client closes it
ff mcp --session flight-3    every child's operation carries the tag
ff hook claude               register the server with Claude Code
ff doctor                    the mcp row says which clients have it
```
