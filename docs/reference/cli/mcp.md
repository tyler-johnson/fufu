# ff mcp

A Model Context Protocol server on stdin and stdout, for an agent client that wants fufu as a tool rather than as a shell command. It exposes one tool, `ff`, whose input is the command line after `ff` as an array of words: `{"args": ["commit", "-m", "parser: skeleton"]}`. Every call runs this same binary as a child with `--json` and hands back the envelope, so capture, `fufu.gitPolicy`, sessions, error ids, and the no-prompt guarantee all hold — the child is an ordinary invocation, and the server adds nothing to it.

One tool instead of one per verb, because the client transmits every tool's description on every turn and shows the model only the first two thousand characters or so of each. This one is a card under that cut — how to call it, the doctrine, every verb by name in `ff --help`'s groups, and a digest of recovery and the landmines — where forty typed tools would be forty cards and a second spelling of the CLI to keep in step. The verb list is walked from the same table `ff --help` reads, so it cannot drift; the rest is one call away, `ff help <verb>` through the tool and the shipped skill where the client has one.

Seven verbs are not offered: `git`, `update`, `watch`, `hook`, `unhook`, `mcp`, and `extension`. Each owns its stream, talks a person through something, or wires the machine, and none of them makes sense inside a tool call. `extension` is the one that is more than a bad fit: the registry it writes is the allowlist for everything fufu says about an extension, so declaring is a person's decision about a machine. Asking for one returns `usage/mcp-verb-unavailable` and names a shell as the place to run it.

An extension is served when it is declared. [`ff extension add <name>`](extension-add.md) records the manifest, and from then on `ff <name>` through the tool is relayed the way a verb is: the child dispatches to `ff-<name>` and the envelope it printed comes back as structured content. An `ff <name>` nobody declared is refused with `usage/mcp-extension-undeclared`, whose exit is `ff extension add <name>`, and a shell is where it runs — `fufu.toolPolicy` lets an undeclared one through there for exactly that reason, so between the two there is always one place it runs. A declared extension whose manifest says `undoable: false` is refused as well, with `usage/mcp-extension-not-undoable`: the tool's annotations say that nothing it serves is destructive, which is honest only of an extension whose writes [`ff undo`](undo.md) takes back, and an MCP server of the extension's own is where it writes its own annotations. Every declared extension is named on the card, `Extensions: tower (next, file, done, …)`, built from the manifest's verb list and capped in both directions so a long registry cannot push the card past the client's cut.

A call may carry `cwd`, the directory to run in, for a client that works across repositories. Without it, the child runs where the server was started, which is the directory the client launched it in. `--session <name>` on `ff mcp`, or `FF_SESSION` in its environment, tags every operation the server's children record, which is how an agent's work stays separable from a person's.

[`ff hook <client>`](hook.md) registers the server with claude, codex, cursor, or gemini, alongside the capture hook it already wires; [`ff unhook <client>`](unhook.md) removes it, and [`ff doctor`](doctor.md) reports it. The two mechanisms do different jobs: the hook snapshots before every tool call the agent makes, whatever tool that is, while the server only sees fufu verbs. Wire both. They also talk: while it serves, the server holds a presence marker under the user cache directory, keyed by the client process that spawned it and by the name it is registered under, and that is what lets the hook refuse `ff` in the shell under `fufu.toolPolicy` only while the tool is actually up — a marker nobody holds counts for nothing. Only fufu's own marker is read there: a declared extension's own server is a process the client starts and fufu never sees, so a registration on disk says it is installed and nothing says it is running. That refusal follows what the tool serves: a builtin verb and a declared extension are both refused and pointed at the tool, while the seven shell-only verbs pass, and so does any `ff <name>` the tool will not serve — one nobody declared, and one declaring `undoable: false` — since a shell is the only place either of those runs.

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
