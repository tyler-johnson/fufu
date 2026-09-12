# ff switch

Branches without the stash dance. Whatever is open here stays with the branch you are leaving as its open commit — the `@` row's sha, at `refs/fufu/open/<branch>`, where `git log --all` shows it — and whatever was parked where you are going comes back exactly as you left it — same files, same edits, same pending description. Both halves are reported, so you always know where your work went and what came back. `ff sw` is the short spelling.

A parked change comes back over a tip that moved by replaying its one commit there, same change id. When that replay conflicts, the switch still happens and the branch is held: exit 3, [`ff status`](status.md) says so, [`ff resolve`](resolve.md) lays the change into the working copy with markers, and `ff resolve --abandon` drops it. The index is not carried: a staged hunk comes back as an unstaged edit.

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
ff undo                        changed your mind: the move rolls back,
                               and both branches' open changes with it
```
