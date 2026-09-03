# ff describe

The open change carries a description before it is ever a commit, so you can name work while you are doing it and let [`ff commit`](commit.md) pick the name up when it closes. -m sets it inline; the bare form opens $EDITOR seeded with the current text — the same spawn git makes for a commit message, and one of the very few fufu makes at all.

-b names the branch you are on instead — the same act whether it is an anonymous petname earning a real name or a chosen name being replaced. The capture chain, any parked change, and the pending description all come along, which is the part a bare `git branch -m` would orphan.

Naming a revision rewords a commit that has already closed instead. Everything above it re-parents in the same operation, so any branches sitting inside that range come along with it. A reword authors a message for a commit, so your `prepare-commit-msg` and `commit-msg` hooks run over it and a hook that exits non-zero refuses the reword before anything is planned; `--no-verify` skips `commit-msg`. No tree moves, so `pre-commit` does not run. The bare form writes a pending description rather than a commit, and runs no hook at all — they fire when the change closes.

The branches stacked on this one follow a reword. Once the reword has landed, every local branch whose base resolves to the reworded branch is replayed onto its new tip, parent before child, in the same operation, so one [`ff undo`](undo.md) takes the cascade back with the reword. A reword moves no tree, so the replay above never conflicts. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named, with everything above it left alone.

## Usage

```
Usage: ff describe [OPTIONS] [rev]

Arguments:
  [rev]
          The revision to reword; omitted describes the open change

Options:
  -m <msg>
          The description text; omitted opens $EDITOR

  -b <branch>
          Name the branch you are on instead — anonymous or already named

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
ff describe -m "parser: handle unicode escapes"
ff describe                    open $EDITOR on the pending description
ff describe -b unicode-cleanup name the branch you are on
ff describe HEAD~2 -m "fix"    reword a closed commit, restacking above it
```
