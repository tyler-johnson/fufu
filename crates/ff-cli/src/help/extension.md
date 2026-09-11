`ff <name>` already runs `ff-<name>` from PATH when no built-in verb matches, and that is the whole of an undeclared extension. Declaring is the missing half: `ff extension <name>` asks the binary for its manifest, checks the contract it claims against this fufu's, and records it; `ff extension -d <name>` takes the name back off; bare `ff extension` is the list.

What declaring buys is that fufu will describe the extension:

- `ff help <name>` and `ff explain <name>/<id>` reach the binary
- its briefing line rides fufu's, and `ff hook` asks the binary for its skills and installs them beside fufu's
- the agent event fans out to it

It buys the extension no capability and no environment — an undeclared `ff-<name>` runs from a shell exactly as it always did, on the same three variables.

The list lives under your config directory rather than in a repository, because the binary is on PATH and declaring it is a decision about this machine. It is also why no agent tool reaches this verb: the list is the allowlist for everything above, so putting a name on it stays a person's gesture.

### Declaring

Runs `ff-<name> --ff-manifest` and reads the one envelope it prints: the verbs the extension answers to, whether its writes are undoable, which is reported and not enforced, and the contract it speaks. The flag is recognized before anything else on the command line, and answers outside a repository.

Three checks stand between the answer and the record:

- the manifest parses as the machine surface types it
- the contract it claims is the one this fufu speaks
- the name it gives is the name of the binary that was resolved

A manifest is refused whole rather than in part, and a refusal records nothing.

Declaring the same name again replaces the record and keeps its place in the order, which is the order subscribers are fanned out in.

What is recorded is the manifest as it was read, unknown fields and all, plus the path the walk landed on and the time; `ff doctor` compares a binary against those to report drift. The path is evidence rather than a route: dispatch stays the PATH walk, so a binary that moves is still found.

### The list

One row per declared extension — its name, the version recorded when it was declared, and the verbs it answers to — in the order they were declared, which is the order subscribers are fanned out in.

A row whose binary has left PATH says so and stays: dispatch is a fresh walk every time, so a name that resolves to nothing today is a fact about PATH rather than a reason to forget the declaration.

Two things can appear below the rows:

- A record claiming a contract this fufu does not speak. It is listed apart, and it is described to nobody.
- A registry file that does not read as one. That is a warning rather than a failure: the listing is empty, nothing on this machine is described while it reads that way, and `ff doctor` names the file.

### Removing

Takes the name off the list. fufu stops describing the extension — `ff help <name>` stops reaching the binary, its briefing line and its skills and its subscriptions all stop being fufu's business.

Nothing is uninstalled. `ff-<name>` is still on PATH and `ff <name>` still runs it, on the same three variables it always had. Skills a `ff hook` install already wrote stay where they were written; the next `ff hook claude` sweeps them from the plugin, and Codex's stay until the next install stops carrying them.

A name that was never declared is refused rather than answered as done. `ff undo` does not reach any of this either: the list is per machine and lives outside every repository, so the way back is `ff extension <name>` again.

## Examples

```
ff extension                 what this machine declares
ff extension --json          the manifests as they were recorded
ff extension tower           ask ff-tower what it is, and record it
ff extension -d tower        take it back off; ff-tower still runs
ff hook claude               its skills go in with fufu's
ff doctor                    every ff-<name> on PATH, declared or not
```
