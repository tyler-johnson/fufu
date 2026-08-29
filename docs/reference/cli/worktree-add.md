# ff worktree add

A second checkout of the same repository: one object store and one ref namespace are shared, and the working tree, the index, and HEAD are what is new.

The chain floor is laid as the worktree is made, so ff undo works there from the first command. A checkout written by hand gets its floor on its first fufu command instead, and undo in it is blind until then.

The branch is a name you give, or a new branch named after the directory when you do not say, or a minted name when that name is taken. A branch open in another worktree is refused: git allows a branch in one tree, and fufu enforces it.

## Usage

```
Usage: ff worktree add [OPTIONS] <path> [branch]

Arguments:
  <path>
          Where to put it

  [branch]
          The branch it stands on — a new one named after the directory if you do not say

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
ff worktree add bay        a second checkout, on a branch of its own
ff worktree add bay side   the same, on a branch that already exists
ff branch list             the branch it now stands on
ff undo                    the checkout and the new branch come back
```
