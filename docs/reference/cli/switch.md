# ff switch

Branches without the stash dance. Whatever is open here is parked with the branch you are leaving, and whatever was parked where you are going comes back exactly as you left it — same files, same edits, same pending description. Both halves are reported, so you always know where your work went and what came back. `ff sw` is the short spelling.

The target is a branch name, or any unique prefix of one. An ambiguous prefix is an error that lists the candidates.

## Usage

```
Usage: ff switch [OPTIONS] <branch>

Arguments:
  <branch>
          Branch name, or a unique prefix of one

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
ff switch main
ff switch uni                  a unique prefix is enough
ff undo                        changed your mind: the park and the move
                               both roll back together
```
