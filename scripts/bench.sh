#!/usr/bin/env bash
# Phase 0 latency proof: ff status / ff log vs the warm git binary on a
# 5000-file repo with backdated mtimes (racy-clean killed, so both sides
# measure pure lstat-compare, not mass rehashing).
#
# Uses hyperfine when installed; otherwise a 20-iteration timing loop.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
FF="$ROOT_DIR/target/release/ff"

if [[ ! -x "$FF" ]]; then
    echo "building release binary..." >&2
    (cd "$ROOT_DIR" && cargo build --release -q)
fi

# Hermetic git environment.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=bench GIT_AUTHOR_EMAIL=bench@bench.test
export GIT_COMMITTER_NAME=bench GIT_COMMITTER_EMAIL=bench@bench.test
export GIT_AUTHOR_DATE="@1600000000 +0000" GIT_COMMITTER_DATE="@1600000000 +0000"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
REPO="$WORK/repo"
mkdir -p "$REPO"
cd "$REPO"

echo "== building 5000-file fixture repo =="
git init -q -b main
for d in $(seq 0 49); do
    mkdir -p "dir$d"
    for f in $(seq 0 99); do
        echo "content $d $f" > "dir$d/f$f.txt"
    done
done
# Backdate before add so the index records old mtimes: no racy-clean skew.
find . -path ./.git -prune -o -type f -print0 | xargs -0 touch -t 202001010000.00
git add -A
git commit -qm "5000 files"
echo "history 0" > note.txt
git add note.txt
git commit -qm "history commit 0"
for i in $(seq 1 30); do
    echo "history $i" > note.txt
    GIT_AUTHOR_DATE="@$((1600000000 + i * 60)) +0000" \
    GIT_COMMITTER_DATE="@$((1600000000 + i * 60)) +0000" \
        git commit -qam "history commit $i"
done
touch -t 202001010000.00 note.txt

# Rows are measured in interleaved rounds so scheduler/thermal drift on this
# box hits every row equally instead of biasing whichever ran last.
declare -a NAMES CMDS
row() {
    NAMES+=("$1")
    shift
    CMDS+=("$(printf '%q ' "$@")")
}

run_rows() {
    local n=${#NAMES[@]} rounds=4 iters=5 i r k s e
    declare -a total
    for ((i = 0; i < n; i++)); do
        eval "${CMDS[$i]}" > /dev/null 2>&1 || { echo "FAILED: ${CMDS[$i]}" >&2; return 1; }
        total[i]=0
    done
    if command -v hyperfine > /dev/null; then
        local args=()
        for ((i = 0; i < n; i++)); do args+=(-n "${NAMES[$i]}" "${CMDS[$i]}"); done
        hyperfine --warmup 3 "${args[@]}"
    else
        for ((r = 0; r < rounds; r++)); do
            for ((i = 0; i < n; i++)); do
                s=$(date +%s%N)
                for ((k = 0; k < iters; k++)); do eval "${CMDS[$i]}" > /dev/null 2>&1; done
                e=$(date +%s%N)
                total[i]=$((total[i] + e - s))
            done
        done
        for ((i = 0; i < n; i++)); do
            awk -v ns="${total[$i]}" -v c=$((rounds * iters)) -v name="${NAMES[$i]}" \
                'BEGIN { printf "%-34s %8.2f ms/run\n", name, ns / c / 1e6 }'
        done
    fi
    NAMES=()
    CMDS=()
}

run_suite() {
    row "git status --porcelain (floor)" git --no-optional-locks status --porcelain
    row "ff status" "$FF" status
    row "ff status --json" "$FF" status --json
    run_rows
}

echo
echo "== clean tree (5000 files) =="
run_suite

echo
echo "== dirty tree (1 modified + 1 untracked) =="
echo changed >> dir0/f0.txt
echo new > untracked.txt
run_suite

echo
echo "== snapshot (bare ff is the verb) =="
# The dirty state above was already captured by the ff status rows
# (capture-first), so repeated bare ff here is the tier-2 no-op steady state:
# full scan + rehash of the dirty files + tree build, ref untouched.
row "ff (tier-2 dirty no-op)" "$FF"
row "git status --porcelain (floor)" git --no-optional-locks status --porcelain
run_rows

# Create: each run records a genuinely new state, so runs are timed one at a
# time around a fresh mutation. Budget: tier-2 + 3ms.
iters=10
total=0
for i in $(seq $iters); do
    echo "create $i" >> dir0/f0.txt
    s=$(date +%s%N)
    "$FF" > /dev/null
    e=$(date +%s%N)
    total=$((total + e - s))
done
awk -v ns="$total" -v c="$iters" \
    'BEGIN { printf "%-34s %8.2f ms/run\n", "ff (create, 1 modified file)", ns / c / 1e6 }'

# Tier-1: clean tree whose state the chain already holds — the scan proves
# emptiness and nothing else runs. Commit the bench dirt so the cache tree is
# valid again (checkout would invalidate it and degrade every scan equally).
# Budget: ff status + 1ms.
git add -A
git commit -qm "bench steady state"
touch -t 202001010000.00 dir0/f0.txt untracked.txt
"$FF" > /dev/null
row "ff (tier-1 clean no-op)" "$FF"
row "ff status (reference)" "$FF" status
run_rows

echo
echo "== first capture of a 5000-file untracked tree (filter-pipeline probe) =="
PROBE="$WORK/probe"
mkdir -p "$PROBE"
cd "$PROBE"
git init -q -b main
for d in $(seq 0 49); do
    mkdir -p "pd$d"
    for f in $(seq 0 99); do
        echo "probe $d $f" > "pd$d/f$f.txt"
    done
done
s=$(date +%s%N)
"$FF" > /dev/null
e=$(date +%s%N)
awk -v ns=$((e - s)) \
    'BEGIN { printf "%-34s %8.2f ms (one shot)\n", "ff (5000 untracked, first snap)", ns / 1e6 }'
cd "$REPO"

echo
echo "== phase 2 verbs (5000 files) =="
git config user.name bench
git config user.email bench@bench.test
git branch bench-other

# Close: each run commits a genuinely new change.
iters=5
total=0
for i in $(seq $iters); do
    echo "close $i" >> dir1/f1.txt
    s=$(date +%s%N)
    "$FF" commit -m "bench close $i" > /dev/null
    e=$(date +%s%N)
    total=$((total + e - s))
done
awk -v ns="$total" -v c="$iters" \
    'BEGIN { printf "%-34s %8.2f ms/run\n", "ff commit (1 modified file)", ns / c / 1e6 }'

# Clean switch round trip (HEAD, index, worktree transition; no park).
iters=5
total=0
for i in $(seq $iters); do
    s=$(date +%s%N)
    "$FF" switch bench-other > /dev/null
    "$FF" switch main > /dev/null
    e=$(date +%s%N)
    total=$((total + e - s))
done
awk -v ns="$total" -v c=$((iters * 2)) \
    'BEGIN { printf "%-34s %8.2f ms/run\n", "ff switch (clean, per leg)", ns / c / 1e6 }'

# Dirty switch round trip: park on the way out, arrive on the way back.
iters=5
total=0
for i in $(seq $iters); do
    echo "park me $i" >> dir2/f2.txt
    s=$(date +%s%N)
    "$FF" switch bench-other > /dev/null
    "$FF" switch main > /dev/null
    e=$(date +%s%N)
    total=$((total + e - s))
    git checkout -q -- dir2/f2.txt
done
awk -v ns="$total" -v c="$iters" \
    'BEGIN { printf "%-34s %8.2f ms/run\n", "ff park+arrive (round trip)", ns / c / 1e6 }'

# Reconcile overhead: the clean path runs inside every verb; measure it via
# the status verb against the phase-1 style capture-only floor (bare ff).
row "ff status (capture+reconcile)" "$FF" status
row "ff (capture only)" "$FF"
run_rows

echo
echo "== log -n 25 =="
row "git log -25 (floor)" git --no-optional-locks log -25 --format='%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s'
row "ff log -n 25" "$FF" log -n 25
row "ff log -n 25 --json" "$FF" log -n 25 --json
run_rows

echo
echo "binary: $(du -h "$FF" | cut -f1) ($FF)"
