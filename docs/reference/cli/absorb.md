# ff absorb

Folds the open change into a commit that has already closed — the revision you name, or the one it sits on when you name none. An absorb does not attribute hunks: the change is the unit, and a path filter only chooses which of its files fold in, leaving the rest open.

Everything above the target re-parents in the same operation, so a branch inside that range comes along with it. What moves is the commit's identity and the stack above it; no file is copied or renamed in the re-point.

The branches stacked on this one follow it. Once the absorb has landed, every local branch whose base resolves to the rewritten branch is replayed onto its new tip, parent before child, in the same operation, so one [`ff undo`](undo.md) takes the cascade back with the absorb. A branch above whose replay conflicts is held on its own, with everything above it left alone, and the absorb still lands; [`ff status`](status.md) shows the branch waiting. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named.

The content is about to become commit content, so your `pre-commit` hook runs over it exactly as it would for a close — the index is staged with what is folding in, and a hook that exits non-zero refuses the absorb. `--no-verify` skips it. No message hook runs: the target keeps its own description untouched.

## Usage

```
Usage: ff absorb [OPTIONS] [path]...

Arguments:
  [path]...
          Limit the absorb to these paths (files or directory prefixes)

Options:
      --into <rev>
          Commit to absorb into; without it, the commit under the change

      --no-verify
          Skip pre-commit and commit-msg hooks

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
ff absorb                      fold everything open into the commit under it
ff absorb --into HEAD~2        fold it into a commit further back
ff absorb src/parser.rs        fold only that path
ff absorb --no-verify          fold without running the pre-commit hook
```
