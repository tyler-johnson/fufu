# ff log

The changes view, jj-style: the open change (@) sits atop the commit walk (●), and each commit wears the id of its newest operation — the letters column `ff evolog` drills into.

--commits drops to plain history, no operation identity. The operation log itself is `ff op log`: every mutation fufu has made, newest first, carrying the ids the `ff op` verbs take.

-r takes a revset and replaces where the rows come from: gitrevisions' whole revision grammar, plus a set algebra spelled | & ~ .. and :: . The @ row appears only when the open change is a member of the set, because `ff log -r main` is a question about main.

Paths narrow the log to the commits that touch them, by the rule `ff restore` speaks: a file, or a directory prefix. No globs.

No `--` is needed, the opposite of what git teaches: revisions go to -r and the positional is only ever paths, so `ff log main` is a question about the path main, even where a branch called main exists.

A path that names a blob is followed through its renames, on by default. A directory gets no follow — git tracks no such thing as a directory rename, so there is nothing to follow.

-r filters but does not follow: a revset names a set, and a set has no line of descent to carry a name along. `ff log -r 'trunk..@' src/` is a good question and still works — it filters.

The @ row appears when the open change touches the paths, the same rule -r already has.

--signatures verifies each row and puts git's status letter beside it — G good, B bad, U untrusted, X expired, Y expired key, R revoked key, E unverifiable, N unsigned. It is opt-in because verifying a page is one signer run per row; without it `ff log` verifies nothing and runs nothing.

The log family pages on a terminal, git-style — fufu.pager, then FF_PAGER, then PAGER, then less. Piped output and --json never page.

## Usage

```
Usage: ff log [OPTIONS] [path]...

Arguments:
  [path]...
          Files or directories to limit the log to; all of them when omitted

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

  -r, --revisions <revset>
          Revisions to show, as a revset; without it, the walk from HEAD

      --commits
          Commits only — the plain history view

      --json
          Emit machine-readable JSON

      --signatures
          Verify each commit's signature and show the status letter — one signer run per row

      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff log                         the last 25 rows
ff log -n 0                    all of it
ff log --commits               history only, no operation rows
ff log --signatures            verify each row and show its status letter
ff log -r main                 just main's tip — no @ row, it is not in it
ff log -r 'trunk..@'           what this branch has that trunk does not
ff log src/parser.rs           what happened to this file, renames and all
ff log -r 'trunk..@' src/      filters that set by path — no rename follow
ff op log                      the operation log, in its own address space
```
