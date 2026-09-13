# ff log

The changes view: the open change (@) sits above the commit walk (●), and each commit has a change ID — the letters column [`ff evolog`](evolog.md) uses to inspect its recorded evolution. The @ row carries the ID the change will keep when [`ff commit`](commit.md) records it in branch history. Its SHA identifies the internal open commit object; a different message, partial commit, signing, or hook can give the recorded commit a different SHA. The SHA is blank while the tree is clean and under signing.

Every commit has a change ID. A commit fufu recorded carries it as a `change-id` header, which jj also uses; surviving changes keep it through fufu rewrites. A commit without that header derives its ID from its SHA, so an outside rewrite can change that identity. The bold prefix is unique only on the displayed page. Resolution requires at least four characters and a unique match in the repository: use more letters for an ambiguous prefix, or a commit SHA for divergent copies of the same change. See [Revisions and IDs](../revisions.md#prefixes-and-divergent-copies) for lookup limits and local/remote precedence.

--commits drops to plain history, no change ids. The operation log itself is [`ff op log`](op-log.md): every mutation fufu has made, newest first, carrying the ids the [`ff op`](op.md) verbs take.

## Choosing the rows

-r takes a revision set and replaces where the rows come from. It accepts commit and change IDs, branch and tag names, supported Git-style suffixes, and the set operators | & ~ .. and :: . The [revision grammar](../revisions.md#revision-sets-and-grammar) lists the implemented forms and boundaries. The @ row appears only when the open change is a member of the set. `@^` is `HEAD`, `@~3` is `HEAD~2`, and on linear history `@~3..@` is two commits plus the open change. It has no reflog, so `@@{1}` is refused; `@{1}` alone is HEAD's.

Paths narrow the log to the commits that touch them, by the rule [`ff restore`](restore.md) speaks: a file, or a directory prefix. No globs. The @ row appears when the open change touches them, the same rule -r has.

No `--` is needed, the opposite of what git teaches: revisions go to -r and the positional is only ever paths, so `ff log main` is a question about the path main, even where a branch called main exists.

## Renames

A path that names a blob is followed through its renames, on by default. A directory gets no follow — git tracks no such thing as a directory rename, so there is nothing to follow.

-r filters but does not follow: a revset names a set, and a set has no line of descent to carry a name along. `ff log -r 'trunk..@' src/` still works — it filters.

## Signatures

A commit that carries a signature says `signed` beside it. That is free — the header is on an object the walk already read — so it is on by default. It is a claim about the commit rather than about the key: a signature is there, not that anybody checked it.

--signatures checks them, replacing `signed` with the verdict, the tool, and the short id of the key — `verified gpg 9B295D68` — or `bad signature`, `untrusted key`, `expired signature`, `expired key`, `revoked key`, `unverifiable`.

The checks run in parallel, one per core up to eight, and cost one signer run per signed row, which is why it is a flag. Unsigned commits say nothing either way.

## Paging

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

      --fetch
          Fetch from the remote first, whatever the cadence says

      --signatures
          Verify each commit's signature and show the status letter — one signer run per row

      --at-op <op>
          Read as of this operation (a hex id or prefix, `@`, `@^`, `@~3`)

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

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
ff log --commits               history only, no change ids
ff log --signatures            verify each row and show its status letter
ff log -r main                 just main's tip — no @ row, it is not in it
ff log -r 'trunk..@'           what this branch has that trunk does not
ff log -r '@~3..@'             the last two commits and the open change
ff log src/parser.rs           what happened to this file, renames and all
ff log -r 'trunk..@' src/      filters that set by path — no rename follow
ff op log                      the operation log, in its own address space
```
