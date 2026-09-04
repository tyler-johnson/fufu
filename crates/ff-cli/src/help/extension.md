`ff <name>` already runs `ff-<name>` from PATH when no built-in verb matches, and that is the whole of an undeclared extension. Declaring is the missing half: `add` asks the binary for its manifest, checks the contract it claims against this fufu's, and records it; `remove` takes the name back off; bare `ff extension` is the list.

What declaring buys is that fufu will describe the extension:

- the `ff mcp` tool serves its verbs, and the card names them
- `ff help <name>` and `ff explain <name>/<id>` reach the binary
- its briefing line rides fufu's, and its skills install beside fufu's with `ff hook`
- the agent event fans out to it

It buys the extension no capability and no environment — an undeclared `ff-<name>` runs from a shell exactly as it always did, on the same three variables.

The list lives under your config directory rather than in a repository, because the binary is on PATH and declaring it is a decision about this machine. It is also the one thing the MCP tool will not do: the list is the allowlist for everything above, so putting a name on it stays a person's gesture.

## Examples

```
ff extension                 what this machine declares
ff extension add tower       ask ff-tower what it is, and record it
ff extension remove tower    take it back off; ff-tower still runs
ff doctor                    every ff-<name> on PATH, declared or not
```
