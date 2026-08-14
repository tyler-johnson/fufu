# Plan: version-swept baselines and a performance trend

**Status: not built.** This is a design worked out on 2026-08-14, after the bench suite landed (8cf969e) and immediately caught `ff log` walking the whole snapshot chain (fixed in 1646e24). It records what the next iteration of this suite should be and, more importantly, the traps that were identified before building it.

The framing: the gate does not need to be strict. It exists to make sure performance is considered at every step, not to be a law.

## Why a fixed threshold is the wrong comparison

`report.py` gates a flat-declared row at 1.5x growth per decade of N. That bound is simultaneously too loose and too tight. A row sitting at 1.0x that drifts to 1.4x is a real regression and sails through. A row that is honestly 1.47x fails nothing today and goes red tomorrow on a contended runner. Neither answers the question anyone actually has, which is whether *this change* made things worse.

## The reframe

Do not compare against a recorded number. Compare against **a binary you rebuild and re-measure right now**.

The machine measures the working tree and the last release back to back, in one session, against one set of fixtures. That removes both problems a committed-number baseline has: staleness, where a figure was recorded before some unrelated change to the box, and portability, where numbers recorded on one machine mean nothing on another. The committed file stops being an input to the check and becomes purely a record.

Ratios are portable across machines — that is the premise the whole suite rests on. Absolute milliseconds are not, and only compare within a single sweep on a single box. A same-session sweep makes milliseconds comparable too, which is what actually answers "is it getting faster".

## Shape

- **Re-measure a window, retain everything.** Each sweep rebuilds and re-measures the last X versions so they are mutually comparable. The committed file keeps every version ever recorded, with older rows flagged as measured then, on that box. Recent history is rigorous; deep history is indicative.
- **No separate time series is needed.** Commit the scores and `git log -p` on that file is the trend, with the commit that moved each number sitting right beside it.
- **Local use is the point**, more than the trend: answering "how does my working tree compare to the last released version". New official numbers get recorded at release.
- **CI stays fast.** Build HEAD — the `test` job already does — plus the last tag, measure both, compare. The tag's build caches by tag, so it is paid once across pull requests rather than per run.
- **Fixture rebuilds are what make it self-healing.** The fixture design changed three times on the day the suite was written, and each change invalidated every earlier number. A re-runnable sweep re-measures the whole window instead, so the trend stays internally consistent rather than carrying a hole where the methodology moved.

## The traps

**The ratchet.** A check that only compares against the last release passes a change that is 10% slower every release, and after five releases you are 60% slower having never once fired. The sweep has already built the whole window, so comparing against the *oldest* version in it costs almost nothing — and is probably the more valuable of the two comparisons.

**Whose fixture?** Asking "did my change make `ff log` faster" wants both versions reading the *same* chain, so that only the read path varies. But an optimization that changes what *capture writes* — exactly what 1646e24 did — measures as no improvement at all on an old-format chain, because there are no segment pointers to skip over. Measuring each version on the chain it produces is the better default: it is what a user of that version actually experiences, the product rather than the diff. The cost is that a capture-format change then appears as a step in the graph which is not a code-speed change, and it has to be labeled or someone will read it as one a year from now. This is not hypothetical — measuring against stale fixtures produced a meaningless result twice while the suite was being built.

**Version compatibility must be declared, not discovered.** Fixture and row definitions come from the working tree, one source of truth. Checking out each tag's own `rows.tsv` sounds more faithful but compares measurements that were defined differently and calls the result a trend. Each version runs the subset it supports.

Split the declaration in two, because the two fail differently: a fixture a version cannot build knocks that version off the axis entirely, while an unsupported row merely empties one cell.

Do not infer support from a non-zero exit. That turns a genuine crash into an empty cell nobody looks at, which is the exact failure class this suite exists to catch. A curated version range is a claim the tooling can check: a row that declares it runs on v0.1.0 and then fails there is an error, loudly.

**Changed semantics are more dangerous than missing commands.** An absent command announces itself. A command present in both versions but doing different work produces a perfectly comparable-*looking* number that is a lie — it shows up as a cliff nobody can attribute. So the column should mean "comparable since", not "exists since": the version from which this row measures the same work. Bump it when behavior changes, and the older cells drop out of the trend rather than lying in it. Print the range beside each row so a gap is visible and attributable.

**Ragged is the feature.** Rows with five versions of history sitting beside rows with one, rendered as gaps, never interpolated or backfilled.

**Noise belongs in the record.** A `report.py` bug that false-failed six rows was invisible on a quiet box and only appeared under contention. A run whose floor measurements were wild should not silently become a data point in the history — record the noise next to the number.

**The toolchain is not pinned per version.** `rust-toolchain.toml` says `stable`, so rebuilding v0.1.0 today builds it with today's compiler. The sweep measures what v0.1.0 costs if you build it now, which folds rustc's improvements in with fufu's own. Worth stating in the output. Pinning per tag is the alternative and this repo does not do it.

## Cost

About 1m50s for a fat-LTO release build on a Raspberry Pi 5, plus fixture build and measurement — call it five minutes per version. One tag exists today, so this is cheap to start and grows slowly with releases. Fine periodically; far too slow to run per pull request.
