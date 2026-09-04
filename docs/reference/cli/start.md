# ff start

Begin a new line of work on a fresh branch. [`ff commit`](commit.md) records, [`ff switch`](switch.md) resumes, `ff start` begins. `ff new` is jj's name for it, and an alias here.

Bare `ff start` forks from trunk; a `<rev>` argument forks there instead. A branch name forks at that branch's tip rather than continuing it — continuing is `ff switch`'s job.

The open change parks where it was; the new branch opens clean. Nothing is ever carried across a fork. -m describes the change being *opened*; -b names the minted branch, else it is anonymous.

`ff start` never creates a commit.

## Usage

```
Usage: ff start [OPTIONS] [target]

Arguments:
  [target]
          Branch, revision, or nothing to stay here

Options:
  -m <msg>
          Pending description for the change being opened

  -b <branch>
          Name for the minted/forked branch (or claim a placeholder)

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
ff start                       begin new work, forked from trunk
ff start -m "the next thing"   …with the new change already described
ff start -b hotfix             name the branch at birth
ff start 5b7a90e               fork from a specific commit
```
