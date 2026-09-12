# ff absorb

Moves content out of a run of commits and into one commit. `ff absorb` and [`ff lift`](lift.md) are one move with two sets of defaults: `--from <revset>` names the sources, `--into <rev>` names the target, and each verb's word is its defaults and nothing more. Absorb moves from the open change into the commit under the sources — fold what is on disk into the commit it belongs to — and every other shape is the same move with an end named. `ff squash` is jj's name for the move, and an alias here.

The sources are one contiguous run of commits on the branch's line, the open change allowed as the top member or alone; `--from HEAD~2..` is the two commits above `HEAD~2`, and they fold into it. The target is any commit on the line — below the run, above it, or a member of it, which takes both sides — or the open change. A move does not attribute hunks: the change is the unit, whole files are what move, and a path filter only chooses which of the sources' files they are, leaving the rest where it was. A source the move empties is dropped, because fufu writes no empty commit, and the report names it.

Everything between and above replays in the same operation, so a branch inside that range comes along with it. What moves is content and the identity of the commits above; no file is copied or renamed in the re-point. A replay that conflicts holds with nothing written, and [`ff resolve`](resolve.md) opens it.

`-m` gives the target a message: a reword for a closed commit, the pending description for the open change. Without it the target keeps what it had.

## Branches stacked above

The branches stacked on this one follow it. Once the move has landed, every local branch whose base resolves to the rewritten branch is replayed onto its new tip, parent before child, in the same operation, so one [`ff undo`](undo.md) takes the cascade back with the move.

A branch above whose replay conflicts is held on its own, with everything above it left alone, and the move still lands; [`ff status`](status.md) shows the branch waiting. A branch checked out in another worktree, one already holding a rewrite, or one whose commits hold a merge is skipped and named.

## Hooks

When the open change is among the sources, its content is about to become commit content, so your `pre-commit` hook runs over it exactly as it would for a close — the index is staged with what is folding in, and a hook that exits non-zero refuses the move. A move between closed commits runs no `pre-commit`. `-m` on a closed commit runs `commit-msg` the way a reword does. `--no-verify` skips both.

## Usage

```
Usage: ff absorb [OPTIONS] [path]...

Arguments:
  [path]...
          Limit the move to these paths (files or directory prefixes)

Options:
      --from <revset>
          Commits to move; without it, the open change

      --into <rev>
          Commit to move into; without it, the commit under the sources

  -m <msg>
          The target's message: a reword for a closed commit, the pending description for the open change

      --json
          Emit machine-readable JSON

      --no-verify
          Skip pre-commit and commit-msg hooks

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff absorb                        fold everything open into the commit under it
ff absorb --into HEAD~2          fold it into a commit further back
ff absorb --from HEAD~2..        fold the two commits above HEAD~2 into it
ff absorb --from HEAD~2.. -m "…" the same, and reword the commit they land in
ff absorb src/parser.rs          fold only that path
ff absorb --no-verify            fold without running the pre-commit hook
```
