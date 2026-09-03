# ff extension

`ff <name>` already runs `ff-<name>` from PATH when no built-in verb matches, and that is the whole of an undeclared extension: fufu found a filename, so a filename is all it can honestly tell an agent about the verb. Declaring is the missing half. `add` asks the binary for its manifest, checks the contract it claims against this fufu's, and records it; `remove` takes the name back off; bare `ff extension` is the list.

What declaring buys is that fufu will describe the extension: the [`ff mcp`](mcp.md) tool serves its verbs, the card names them, `ff help <name>` and [`ff explain <name>/<id>`](explain.md) reach the binary, its briefing line rides fufu's, its skills install beside fufu's with [`ff hook`](hook.md), and the agent event fans out to it. It buys the extension no capability and no environment — an undeclared `ff-<name>` runs from a shell exactly as it always did, on the same three variables.

The list lives under your config directory rather than in a repository, because the binary is on PATH and declaring it is a decision about this machine. It is also the one thing the MCP tool will not do: the list is the allowlist for everything above, so putting a name on it is a person's gesture, not one an agent makes for itself.

## Usage

```
Usage: ff extension [OPTIONS] [COMMAND]

Commands:
  add     Ask an ff-<name> for its manifest, check it, and record it here
  list    Every extension declared on this machine, and what each answers to
  remove  Take one off the list; fufu stops describing it
  help    Print this message or the help of the given subcommand(s)

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
ff extension                 what this machine declares
ff extension add tower       ask ff-tower what it is, and record it
ff extension remove tower    take it back off; ff-tower still runs
ff doctor                    every ff-<name> on PATH, declared or not
```
