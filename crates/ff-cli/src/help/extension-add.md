Runs `ff-<name> --ff-manifest` and reads the one envelope it prints: the verbs the extension answers to, whether its writes are undoable, and the contract it speaks. The flag is recognized before anything else on the command line, and answers outside a repository.

Three checks stand between the answer and the record:

- the manifest parses as the machine surface types it
- the contract it claims is the one this fufu speaks
- the name it gives is the name of the binary that was resolved

A manifest is refused whole rather than in part, and a refusal records nothing.

Declaring the same name again replaces the record and keeps its place in the order, which is the order subscribers are fanned out in.

What is recorded is the manifest as it was read, unknown fields and all, plus the path the walk landed on and the time; `ff doctor` compares a binary against those to report drift. The path is evidence rather than a route: dispatch stays the PATH walk, so a binary that moves is still found.

## Examples

```
ff extension add tower       ask ff-tower what it is, and record it
ff extension list            what this machine declares now
ff hook claude               its skills and its server go in with fufu's
ff extension remove tower    take it back off
```
