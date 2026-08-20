#!/usr/bin/env bash
# Self-test for report.py against the synthetic fixtures in testdata/.
# No test framework: run each fixture, check the exit code, grep the
# output for the row name or verdict that fixture exists to exercise.
set -uo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
BENCH_DIR="$ROOT_DIR/bench"
REPORT="$BENCH_DIR/report.py"
DATA="$BENCH_DIR/testdata"

fail_count=0

# args: name, fixture file, expected exit code, grep pattern expected in output
case_check() {
    local name="$1" fixture="$2" want_exit="$3" pattern="$4"
    local out
    out=$(python3 "$REPORT" "$DATA/$fixture" 2>&1)
    local got_exit=$?

    if [[ "$got_exit" != "$want_exit" ]]; then
        echo "FAIL: $name (exit=$got_exit, want=$want_exit)"
        fail_count=$((fail_count + 1))
        return
    fi
    if ! grep -qF -- "$pattern" <<< "$out"; then
        echo "FAIL: $name (output missing '$pattern')"
        fail_count=$((fail_count + 1))
        return
    fi
    echo "PASS: $name"
}

case_check "flat.json is a clean pass"                 flat.json               0 "PASS: 2 flat ff row(s)"
case_check "linear.json fails and names the row"       linear.json             1 "evolog / chain-depth / ff"
case_check "expect-linear.json is not gated"           expect-linear.json      0 "PASS: no flat rows measured"
case_check "below-floor.json passes as noise"          below-floor.json        0 "below-floor"
case_check "below-floor-grows.json fails"              below-floor-grows.json  1 "hidden-scan / chain-depth / ff"
case_check "missing-floor.json fails"                  missing-floor.json      1 "no-floor"
# A measurement whose own stddev is 2-3x its mean cannot support any verdict.
# These are the real restore-at numbers, which passed at 1.31x on one CI run
# and failed at 5.12x on the next with no code between them. Same ratio as
# linear.json above, which must still fail -- the difference is measurability,
# so this row is withheld rather than scored, and is NOT counted as a pass.
case_check "noisy-measurement.json is withheld"        noisy-measurement.json  0 "[NOISY]"
case_check "noisy-measurement.json is not a pass"      noisy-measurement.json  0 "NOT MEASURED: 1 flat ff row(s)"
# Floors are measured per-N, so floor drift alone can manufacture growth in the
# adjusted numbers. These are the real oplog numbers: the command got FASTER
# (3.15 -> 3.13ms) while its two floors were measured 0.22ms apart, and the
# below-floor branch read that as a suspected linear term.
case_check "floor-drift.json is not cost growth"       floor-drift.json        0 "floor drift, not cost"
case_check "errored.json is reported, not gated"       errored.json            0 "errored"
# The gate is per decade of N, not per span: a row growing 1.35x per decade is
# 1.82x measured across 100 -> 10000, which an endpoint comparison would fail
# even though the cost is plainly sub-linear. Guards that normalization.
case_check "sublinear.json passes on per-decade math"  sublinear.json          0 "PASS: 1 flat ff row(s)"
case_check "expect-report.json is never gated"         expect-report.json      0 "PASS: no flat rows measured"
# A typo in rows.tsv's expect column would silently switch the gate off for
# that row and still print green, so an unknown value must refuse to run.
case_check "bad-expect.json refuses to run"            bad-expect.json         2 "unknown expect 'falt'"
# A noisy small-N floor once made every row -- including ones that got FASTER --
# trip the below-floor "linear term suspected" branch, because the two ends were
# judged against their own point's floor stddev. Growth is required now.
case_check "noisy-floor.json does not false-fail"      noisy-floor.json        0 "noisy floor at n=100"

# Compare mode (--compare BASE HEAD) tests.
case_compare() {
    local name="$1" base="$2" head="$3" want_exit="$4" pattern="$5"
    local out
    out=$(python3 "$REPORT" --compare "$DATA/$base" "$DATA/$head" 2>&1)
    local got_exit=$?

    if [[ "$got_exit" != "$want_exit" ]]; then
        echo "FAIL: $name (exit=$got_exit, want=$want_exit)"
        fail_count=$((fail_count + 1))
        return
    fi
    if ! grep -qF -- "$pattern" <<< "$out"; then
        echo "FAIL: $name (output missing '$pattern')"
        fail_count=$((fail_count + 1))
        return
    fi
    echo "PASS: $name"
}

# Three groups: stable (<2%), slower (+25%), faster (-25%). The ! marker
# appears on the two >=10% changes, and the percentage signs should match.
case_compare "compare: stable/slower/faster groups" \
    "compare-base.json" "compare-head.json" \
    0 "!"

# The slower row: adjusted 5.00 -> 6.25 is +25.0%.
case_compare "compare: slower row shows +25.0%" \
    "compare-base.json" "compare-head.json" \
    0 "+25.0%"

# The faster row: adjusted 5.00 -> 3.75 is -25.0%.
case_compare "compare: faster row shows -25.0%" \
    "compare-base.json" "compare-head.json" \
    0 "-25.0%"

# Base missing a group the head has (evolog not in old binary).
case_compare "compare: missing group names the side" \
    "compare-missing-base.json" "compare-missing-head.json" \
    0 "base: missing"

# Hosts differing between the two files.
case_compare "compare: host difference warning" \
    "compare-hosts-base.json" "compare-hosts-head.json" \
    0 "WARNING: hosts differ"

# stdin ('-') must read identically to a path argument.
out=$(python3 "$REPORT" - < "$DATA/flat.json" 2>&1)
got_exit=$?
if [[ "$got_exit" == "0" ]] && grep -qF "PASS: 2 flat ff row(s)" <<< "$out"; then
    echo "PASS: stdin (-) reads the same as a path"
else
    echo "FAIL: stdin (-) reads the same as a path (exit=$got_exit)"
    fail_count=$((fail_count + 1))
fi

# Compare mode: commands that differ only in the leading binary token
# (against.sh gives the ref binary a version-stamped name) should NOT
# trigger "commands differ".
case_compare_absent() {
    local name="$1" base="$2" head="$3" want_exit="$4" pattern="$5"
    local out
    out=$(python3 "$REPORT" --compare "$DATA/$base" "$DATA/$head" 2>&1)
    local got_exit=$?

    if [[ "$got_exit" != "$want_exit" ]]; then
        echo "FAIL: $name (exit=$got_exit, want=$want_exit)"
        fail_count=$((fail_count + 1))
        return
    fi
    if grep -qF -- "$pattern" <<< "$out"; then
        echo "FAIL: $name (output unexpectedly contains '$pattern')"
        fail_count=$((fail_count + 1))
        return
    fi
    echo "PASS: $name"
}

case_compare_absent "compare: binary-token-only diff hides commands differ" \
    "compare-cmds-base.json" "compare-cmds-head.json" \
    0 "commands differ"

# Commands that differ in their arguments should still show "commands differ".
case_compare "compare: argument diff shows commands differ" \
    "compare-cmds-diff-base.json" "compare-cmds-diff-head.json" \
    0 "commands differ"

if [[ "$fail_count" -eq 0 ]]; then
    echo "all cases passed"
    exit 0
else
    echo "$fail_count case(s) failed"
    exit 1
fi
