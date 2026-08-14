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
    if ! grep -qF "$pattern" <<< "$out"; then
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
case_check "errored.json is reported, not gated"       errored.json            0 "errored"
# The gate is per decade of N, not per span: a row growing 1.35x per decade is
# 1.82x measured across 100 -> 10000, which an endpoint comparison would fail
# even though the cost is plainly sub-linear. Guards that normalization.
case_check "sublinear.json passes on per-decade math"  sublinear.json          0 "PASS: 1 flat ff row(s)"
case_check "expect-report.json is never gated"         expect-report.json      0 "PASS: no flat rows measured"
# A typo in rows.tsv's expect column would silently switch the gate off for
# that row and still print green, so an unknown value must refuse to run.
case_check "bad-expect.json refuses to run"            bad-expect.json         2 "unknown expect 'falt'"

# stdin ('-') must read identically to a path argument.
out=$(python3 "$REPORT" - < "$DATA/flat.json" 2>&1)
got_exit=$?
if [[ "$got_exit" == "0" ]] && grep -qF "PASS: 2 flat ff row(s)" <<< "$out"; then
    echo "PASS: stdin (-) reads the same as a path"
else
    echo "FAIL: stdin (-) reads the same as a path (exit=$got_exit)"
    fail_count=$((fail_count + 1))
fi

if [[ "$fail_count" -eq 0 ]]; then
    echo "all cases passed"
    exit 0
else
    echo "$fail_count case(s) failed"
    exit 1
fi
