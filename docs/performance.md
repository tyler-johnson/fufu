# Performance

The tables below measure selected commands as snapshot-chain depth or commit-history depth increases from 100 to 10,000. They are **recorded results for fufu 0.12.0 (`f9bce73`, September 4, 2026)**, not measurements of the current release. Within these fixtures, the displayed fufu times change little as depth grows; that does not establish constant cost for every command or repository shape.

The existing suite uses hyperfine and a floor-subtracted growth check on fufu rows declared `flat`. Its current default threshold is 1.5× per tenfold increase in the varied count, with noise and below-floor handling in `scripts/bench/report.py`. CI runs a reduced fufu-only matrix at 100 and 1,000 when the Rust workflow runs. This gate checks growth across fixtures; it is not a cross-tool speed gate or an absolute latency budget.

## What it costs

<!-- bench:begin — generated from bench-results/raw.json by scripts/bench/docs-table.py; run make bench, then make bench-docs -->

Measured on Cortex-A76 (aarch64, 4 cores, linux) with hyperfine 1.19.0, against fufu 0.12.0 (f9bce73 2026-09-04), git version 2.50.1, and jj 0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5.

### Snapshot chain depth

n is the number of captures behind the working copy; the fixture varies that count while keeping the working tree small.

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

n is the number of commits on the branch; this measures a bounded log query over increasing history depth.

| operation | fufu runs | n = 100 | n = 1 000 | n = 10 000 | per decade |
|---|---|---|---|---|---|
| log | `ff log -n 25` | 6.5 ms | 7.7 ms | 6.7 ms | 1.04× |

At n = 10 000, against git and jj:

| operation | fufu | git | jj |
|---|---|---|---|
| log | 6.7 ms | 3.1 ms | 33.2 ms |

<!-- bench:end -->

## How to read it

The millisecond cells are **raw mean wall-clock times**, including startup. They estimate command latency on the named machine under this run's conditions. The `per decade` column instead compares endpoint means after subtracting separately measured startup floors, normalized to a tenfold increase in n. About 1.0 means little adjusted growth over this range; about 10 would indicate proportional growth. Small differences near the floor are sensitive to noise.

At n = 10,000, **fufu's displayed mean is lower than jj's on every row with both measurements**, while Git's mean is lower than fufu's wherever Git has a column. For example, snapshot-depth status is 6.8 ms for fufu, 19.5 ms for jj, and 2.0 ms for Git. The tools perform different work: [`ff status`](reference/cli/status.md) includes capture and fufu state, and the output formats differ. These observations apply to the listed commands and versions, not to all workloads. Neither raw times nor ratios are guaranteed to transfer across machines, storage, or cache conditions.

The historical `capture` row runs **bare `ff`**, including its map rendering. Its Git comparison is `git add -A && git commit -m x`, while jj runs `jj status`, which snapshots as part of that command. These are capture-related workloads, not equivalent history updates: a fufu snapshot is an internal operation, while Git's command advances branch history. The current [open-commit model](concepts/changes.md#internal-storage-and-branch-history) arrived after the measured fufu version and should not be used to explain that run's object count.

## What is not flat, and why

Working-copy scanning depends on file count, changed content, and stat-cache validity. The current file-count rows are declared `linear` and reported without the growth gate. History-depth log is also declared `linear`; only the fufu `flat` rows on snapshot-chain depth are growth-gated in the displayed axes.

The published fixtures use a warmed stat cache. Their snapshot chain has one branch and an unchanged base, so it does not exercise realistic segment transitions or a many-branch map. The chain objects are loose; a mixed loose/packed store can have different lookup costs. The table does not measure cold rehashing, a combined large-file/large-history workload, network latency, hooks, signing, every daily verb, or a current-version regression against a rebuilt baseline.

The benchmark rebuild is planned to separate regression checks, cross-tool comparisons, and an absolute wall-clock budget, with realistic chain segments, loose/packed objects, and cold-cache preparation. Its proposed one-second target for bare ff and ff status on a Pi 5 with one million commits, ten million snapshots, and 100,000 files has **not been verified by these tables**. The existing growth gate remains in place until that rebuild ships.

## Reproducing it

```sh
make bench          # default chain/history sweeps, then the growth report
make bench-report   # re-analyze the last run without measuring again
make bench-real     # the same commands against a real public repository
```

These commands measure the current checkout and installed comparison tools; they do not recreate the historical binaries automatically. `scripts/bench/rows.tsv` declares commands, fixture axes, preparation, and expectations. A `-` means the suite defines no comparison for that row. `make bench-docs` renders the saved `bench-results/raw.json` through the report's arithmetic and records its tool/host provenance; it performs no timing run. Keep the raw results with any claimed measurement.
