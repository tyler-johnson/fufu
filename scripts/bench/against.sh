#!/usr/bin/env bash
# against.sh <ref> [run.sh args...]
#
# Measure the working tree against a rebuilt older binary, side by side,
# on the same fixtures on the same machine. A recorded number is always
# stale (another day, another load) and machine-portable (another box),
# so the only honest comparison is two binaries measured back to back now.
#
# Scope: this compares refs whose ff binary can still build bench fixtures.
# Any recent ref works — this is the "did my change make this worse" case
# the script exists for. A ref predating a command the fixture builder needs
# (e.g., v0.1.0 which has no evolog) cannot build fixtures and will fail
# with an explanation rather than a silent exit.
#
# Cost: one full rebuild of <ref> (~2m50s cold on a Pi 5, faster once
# target/against is warm) plus two measurement passes. This is a
# comparison view, not a gate — report.py --compare always exits 0.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
BENCH_DIR="$ROOT_DIR/scripts/bench"

usage() {
    cat <<'EOF' >&2
against.sh <ref> [--axis ...] [--points ...] [--rows ...] [--tools ...]
             [--min-runs N] [--fixtures DIR]

Build <ref> in a worktree, measure it and the working tree back to back,
and print a side-by-side comparison.  --out and --ff-binary are owned by
this script and must not be passed.
EOF
}

# --- Argument parsing ---------------------------------------------------

if [[ $# -lt 1 ]]; then
    echo "error: ref argument required" >&2
    usage
    exit 2
fi

REF="$1"
shift

# --out and --ff-binary are set by this script; forwarding them would
# silently produce wrong output or measure the wrong binary.
for arg in "$@"; do
    if [[ "$arg" == "--out" || "$arg" == --out=* ]]; then
        echo "error: --out is owned by against.sh and must not be passed" >&2
        exit 2
    fi
    if [[ "$arg" == "--ff-binary" || "$arg" == --ff-binary=* ]]; then
        echo "error: --ff-binary is owned by against.sh and must not be passed" >&2
        exit 2
    fi
done

RUN_ARGS=("$@")

# --- Resolve the ref before doing anything expensive --------------------

FULL_SHA=$(git -C "$ROOT_DIR" rev-parse --verify "${REF}^{commit}" 2>/dev/null) || {
    echo "error: cannot resolve ref '${REF}': not a commit" >&2
    exit 2
}
SHORT_SHA="${FULL_SHA:0:7}"

echo "info: resolved ${REF} -> ${SHORT_SHA} (${FULL_SHA})"

# --- Build the working tree explicitly ----------------------------------
# This is the single most damaging failure this script can have: a stale
# target/release/ff from three commits ago measured as "head" makes the
# entire comparison silently wrong. Never rely on run.sh "build if missing".
echo "info: building working tree (head) with cargo build --release"
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"

# --- Build the ref side in a detached worktree --------------------------

WORKDIR=$(mktemp -d)
trap 'git worktree remove --force "$WORKDIR" 2>/dev/null || true' EXIT

git worktree add --detach "$WORKDIR" "$FULL_SHA" || {
    echo "error: cannot create worktree for ${FULL_SHA}" >&2
    rm -rf "$WORKDIR"
    exit 3
}

echo "info: building ref ${SHORT_SHA} in worktree"
BUILD_LOG="$WORKDIR/build.log"
CARGO_TARGET_DIR="$ROOT_DIR/target/against" \
    cargo build --release --manifest-path "$WORKDIR/Cargo.toml" \
    > "$BUILD_LOG" 2>&1 || {
    echo "error: build of ${SHORT_SHA} failed (last 20 lines):" >&2
    tail -20 "$BUILD_LOG" >&2
    echo "error: giving up — an old ref that no longer compiles is an answer, not a bug" >&2
    exit 3
}

# Copy the binary out before the worktree disappears. The copy is what
# makes back-to-back against.sh runs safe — the next ref's build would
# otherwise overwrite the binary at the shared target/against path.
mkdir -p "$ROOT_DIR/bench-results/against"
cp "$ROOT_DIR/target/against/release/ff" "$ROOT_DIR/bench-results/against/ff-${SHORT_SHA}"

# Drop the worktree now; the binary is safe in bench-results/.
git worktree remove --force "$WORKDIR"
trap - EXIT

# --- Measure both sides ------------------------------------------------
# Ref first, head second. Head last leaves the fixture cache in the state
# a plain make bench expects. Both rebuild fixtures (build-id keyed) which
# is expected, not a hang.

FIXTURES_DIR="$ROOT_DIR/bench-fixtures"

echo "info: measuring ref ${SHORT_SHA} (fixture rebuild between sides is expected)"
if ! bash "$BENCH_DIR/run.sh" \
    --ff-binary "$ROOT_DIR/bench-results/against/ff-${SHORT_SHA}" \
    --out "$ROOT_DIR/bench-results/against/${SHORT_SHA}.json" \
    --fixtures "$FIXTURES_DIR" \
    "${RUN_ARGS[@]}"; then
    echo "error: ref ${REF} (${SHORT_SHA}) measurement failed" >&2
    echo "error: the ref's own ff binary builds the ref's own fixtures, so a ref" >&2
    echo "error: predating a command the fixture builder needs cannot be measured;" >&2
    echo "error: see scripts/bench/PLAN.md for the cross-version limitation" >&2
    exit 3
fi

echo "info: measuring head (working tree)"
if ! bash "$BENCH_DIR/run.sh" \
    --ff-binary "$ROOT_DIR/target/release/ff" \
    --out "$ROOT_DIR/bench-results/against/head.json" \
    --fixtures "$FIXTURES_DIR" \
    "${RUN_ARGS[@]}"; then
    echo "error: head (working tree) measurement failed — this is an ordinary bench failure, not a cross-version limitation" >&2
    exit 1
fi

# --- Compare -----------------------------------------------------------

python3 "$BENCH_DIR/report.py" \
    --compare \
    "$ROOT_DIR/bench-results/against/${SHORT_SHA}.json" \
    "$ROOT_DIR/bench-results/against/head.json"

exit $?
