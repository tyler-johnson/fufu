# Performance

fufu's claim is not that it is fast. It is that nothing it adds grows: a verb that costs 7 ms against a hundred snapshots costs 7 ms against ten thousand. Snapshots are what fufu puts into a git repository that git does not, so a chain that made [`ff status`](reference/cli/status.md) a little slower every day would be the honest reason not to use it. That is the number this project gates on.

The claim is measured rather than asserted. `make bench` builds fixtures at 100, 1 000 and 10 000, runs every row of `scripts/bench/rows.tsv` through hyperfine, subtracts a measured process-start floor, and fails when a row declared flat grows more than 1.5× per decade of n. A reduced version of the same matrix runs in CI on every push, so the gate does not depend on anyone remembering to run it.

## What it costs

<!-- bench:begin — generated from bench-results/raw.json by scripts/bench/docs-table.py; run make bench, then make bench-docs -->

Measured on Cortex-A76 (aarch64, 4 cores, linux) with hyperfine 1.19.0, against fufu 0.12.0 (f9bce73 2026-09-04), git version 2.50.1, and jj 0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5.

### Snapshot chain depth

Snapshots are what fufu adds to a git repository, so this is the axis that would sink it: n is the number of captures behind the working copy.

| operation | fufu runs | n = 100 | n = 1 000 | n = 10 000 | per decade |
|---|---|---|---|---|---|
| capture | a bare `ff` | 9.2 ms | 8.7 ms | 7.9 ms | 0.86× |
| evolog | `ff evolog -n 25` | 6.2 ms | 6.9 ms | 6.9 ms | 1.12× |
| log | `ff log -n 25` | 5.9 ms | 5.8 ms | 6.5 ms | 1.12× |
| oplog | `ff op log -n 25` | 6.3 ms | 6.4 ms | 6.8 ms | 1.09× |
| restore-at | `ff restore --all --at-op <op>` | 6.7 ms | 6.7 ms | 7.1 ms | 1.05× |
| status | `ff status` | 6.7 ms | 6.7 ms | 6.8 ms | 1.01× |

At n = 10 000, against git and jj:

| operation | fufu | git | jj |
|---|---|---|---|
| capture | 7.9 ms | 6.1 ms | 31.7 ms |
| evolog | 6.9 ms | — | 58.1 ms |
| log | 6.5 ms | 1.9 ms | 22.0 ms |
| oplog | 6.8 ms | — | 18.3 ms |
| restore-at | 7.1 ms | — | — |
| status | 6.8 ms | 2.0 ms | 19.5 ms |

### Commit history depth

n is the number of commits on the branch — the axis git itself is measured on.

| operation | fufu runs | n = 100 | n = 1 000 | n = 10 000 | per decade |
|---|---|---|---|---|---|
| log | `ff log -n 25` | 6.5 ms | 7.7 ms | 6.7 ms | 1.04× |

At n = 10 000, against git and jj:

| operation | fufu | git | jj |
|---|---|---|---|
| log | 6.7 ms | 3.1 ms | 33.2 ms |

<!-- bench:end -->

## How to read it

The milliseconds are this machine's and mean nothing on yours. Ratios are what port between machines, and the ratio is what the suite gates on: the `per decade` column is the floor-subtracted growth per 10× of n, so flat is about 1.0 and linear would be about 10.

git is faster on a plain read, and that is the shape of the trade rather than a defect. [`ff status`](reference/cli/status.md) reads the operation log and the snapshot chain as well as the working copy, and `git status` reads the tree; the tables say the difference is a small constant that does not open up as a repository ages. Against jj, which snapshots the working copy on every command the way fufu does, the same operations run three to eight times slower on this box.

A capture is not a commit. The `capture` row is fufu's snapshot of the working copy — the thing that happens before every operation and every agent tool call — and it is measured against `git add -A && git commit`, which is the closest git has.

## What is not flat, and why

Scanning the working copy is O(files), for fufu exactly as for git: [`ff status`](reference/cli/status.md) on a tree of fifty thousand files costs more than on a tree of five hundred, and nothing in the design pretends otherwise. `scripts/bench/rows.tsv` declares those rows `linear` rather than `flat`, and they are measured for visibility, never gated. The first capture of a repository is the same story — it reads every file once, because it has to.

What is gated is everything that could have been made to scale with the history: reading the log, reading the operation log, restoring a file from an old operation, and taking the snapshot itself.

## Reproducing it

```sh
make bench          # the two gated axes, then the report
make bench-report   # re-analyze the last run without measuring again
make bench-real     # the same commands against a real public repository
```

`scripts/bench/rows.tsv` is the declared table: every row names the operation, the axis it varies, whether it is gated, and the command each of the three tools runs. A `-` in a column means that tool has no honest equivalent for that row, not that measuring it was forgotten. `make bench-docs` regenerates the tables above from the last run.
