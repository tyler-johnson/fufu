#!/usr/bin/env bash
# The measurement half of the bench suite: builds fixtures, runs the declared
# rows.tsv table through hyperfine, and writes bench-results/raw.json in the
# schema frozen alongside this script. No pass/fail judgement happens here --
# that is report.py's job, working from the numbers this script hands it.
#
# fufu's whole performance claim is "no linear-time costs": a command that is
# 6ms today should be 6ms at 100x the history. That only shows up as a ratio
# (N vs 10N vs 100N), never as an absolute number, so everything below is
# built around reusable per-axis fixtures and a floor measurement to subtract
# out process-startup noise -- see bench-schema.md for why.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

AXIS=all
POINTS_ARG=""
ROWS_ARG=""
TOOLS_ARG="ff,git,jj"
OUT_PATH="bench-results/raw.json"
MIN_RUNS=10
FIXTURES_DIR="bench-fixtures"
KEEP_GOING=0
REAL_REPO_ARG="git"
REAL_SHA_ARG=""
FF_BINARY_ARG=""

# FIXTURE_STAMP_FORMAT is the version of the fixture stamp format on disk.
# Every stamp string written or compared in this file carries it as its first
# field. Bump this number whenever a fixture's construction changes in a way
# that would make an on-disk fixture unsafe to reuse. The split of the
# real-history base (git becomes pristine, ff is derived from it) is such a
# change: without the bump, git and jj fixtures would be reused in their old
# fufu-metadata-polluted shape.
FIXTURE_STAMP_FORMAT=2

usage() {
    cat <<'EOF' >&2
run.sh [--axis chain-depth|history-depth|real-history|file-count|all] [--points 100,1000,10000]
       [--rows name[,name...]] [--tools ff[,git,jj]] [--out PATH]
       [--min-runs N] [--fixtures DIR] [--keep-going] [--ff-binary PATH]
       [--real-repo alias|url] [--real-sha SHA]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --axis) AXIS=$2; shift 2 ;;
        --points) POINTS_ARG=$2; shift 2 ;;
        --rows) ROWS_ARG=$2; shift 2 ;;
        --tools) TOOLS_ARG=$2; shift 2 ;;
        --out) OUT_PATH=$2; shift 2 ;;
        --min-runs) MIN_RUNS=$2; shift 2 ;;
        --fixtures) FIXTURES_DIR=$2; shift 2 ;;
        --keep-going) KEEP_GOING=1; shift ;;
        --real-repo) REAL_REPO_ARG=$2; shift 2 ;;
        --real-sha) REAL_SHA_ARG=$2; shift 2 ;;
        --ff-binary) FF_BINARY_ARG=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "run.sh: unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

case "$AXIS" in
    chain-depth) AXES=(chain-depth) ;;
    history-depth) AXES=(history-depth) ;;
    real-history) AXES=(real-history) ;;
    file-count) AXES=(file-count) ;;
    # real-history clones a real public repo over the network and is reported,
    # never gated (see rows.tsv) -- CI, and anyone running the hermetic suite
    # offline, must never trip over it by accident. file-count is excluded
    # for a different reason: its rows are all "linear" too (also never
    # gated), and its own default points run up to 50000 files, slow and
    # large to build the first time. "all" stays the two synthetic, gated
    # axes; real-history and file-count each run only when named explicitly.
    all) AXES=(chain-depth history-depth) ;;
    *) echo "run.sh: --axis must be chain-depth, history-depth, real-history, file-count, or all" >&2; exit 2 ;;
esac

# Repository aliases for the real-history axis: URL plus a pinned commit SHA,
# verified live on 2026-08-14. Pinning is what makes the fixture reproducible
# -- an unpinned --depth N clone of a moving default branch produces
# different numbers every week, with no way to tell that apart from a real
# regression. Re-verify (and update the date above) if an alias ever 404s.
declare -A REAL_REPO_URL_TABLE=(
    [git]="https://github.com/git/git.git"
    [linux]="https://github.com/torvalds/linux.git"
    [cargo]="https://github.com/rust-lang/cargo.git"
    [jj]="https://github.com/jj-vcs/jj.git"
)
declare -A REAL_REPO_SHA_TABLE=(
    [git]="745601a9a94110d74769ab605ccd4f61339758d2"
    [linux]="a5161661ae99f497affa83a5b8654e457cda6267"
    [cargo]="a9792b43fe33dee757959c3c396035b63f8b5950"
    [jj]="7fa941edb45b62efdadff6b01f6f8674dbad9063"
)

REAL_REPO_ALIAS="" REAL_REPO_URL="" REAL_REPO_SHA=""
if [[ " ${AXES[*]} " == *" real-history "* ]]; then
    if [[ -n "${REAL_REPO_URL_TABLE[$REAL_REPO_ARG]+_}" ]]; then
        REAL_REPO_ALIAS=$REAL_REPO_ARG
        REAL_REPO_URL=${REAL_REPO_URL_TABLE[$REAL_REPO_ARG]}
        REAL_REPO_SHA=${REAL_REPO_SHA_TABLE[$REAL_REPO_ARG]}
        # --real-sha overrides the pin even for a known alias, e.g. to try a
        # newer commit of the same repo without editing this table.
        [[ -n "$REAL_SHA_ARG" ]] && REAL_REPO_SHA=$REAL_SHA_ARG
    else
        if [[ -z "$REAL_SHA_ARG" ]]; then
            echo "run.sh: --real-sha is required when --real-repo is a raw URL" >&2
            exit 2
        fi
        REAL_REPO_ALIAS=$(basename "$REAL_REPO_ARG" .git)
        REAL_REPO_URL=$REAL_REPO_ARG
        REAL_REPO_SHA=$REAL_SHA_ARG
    fi
fi

IFS=',' read -r -a TOOLS <<<"$TOOLS_ARG"

if [[ -n "$POINTS_ARG" ]]; then
    IFS=',' read -r -a POINTS <<<"$POINTS_ARG"
elif [[ "$AXIS" == "real-history" ]]; then
    # One point, not three. --depth bounds generations from the tip, not the
    # number of commits fetched: one merge drags its whole side branch in at
    # the depth the merge itself sits, so on a merge-heavy repo the count runs
    # far past the depth asked for -- git/git at depth 500 yields all ~81k of
    # its commits. Depth is therefore a knob for how much to download, never a
    # scale to slope against, and this axis exists to show the synthetic axes'
    # claim holding on a repository a reader can clone, not to gate anything.
    # 1000 is deep enough to be the whole history of a mid-size project and
    # shallow enough to stay tractable on something the size of the kernel.
    POINTS=(1000)
elif [[ "$AXIS" == "file-count" ]]; then
    # 5000 is bench.sh's own fixture size, so the middle point reproduces
    # today's coverage; 500 and 50000 bracket it by 10x each way.
    POINTS=(500 5000 50000)
else
    POINTS=(100 1000 10000)
fi

# git has nothing that plays the role of chain-depth (see rows.tsv's header
# comment and the plan brief): there is no automatically-recorded
# working-tree state in git to grow. So git is only ever measured once on
# that axis, at the largest point, as a reference number for "what does
# this cost in git at all" -- never as a claim about scaling. POINTS_MAX
# is that cutoff.
POINTS_MAX=${POINTS[0]}
for _p in "${POINTS[@]}"; do
    (( _p > POINTS_MAX )) && POINTS_MAX=$_p
done
unset _p

[[ "$OUT_PATH" = /* ]] || OUT_PATH="$ROOT_DIR/$OUT_PATH"
[[ "$FIXTURES_DIR" = /* ]] || FIXTURES_DIR="$ROOT_DIR/$FIXTURES_DIR"
OUT_DIR=$(dirname "$OUT_PATH")
RAW_DIR="$OUT_DIR/raw"
MANIFEST="$OUT_DIR/manifest.jsonl"
mkdir -p "$OUT_DIR" "$RAW_DIR" "$FIXTURES_DIR"
: > "$MANIFEST"

info() { echo "$@" >&2; }

# Compute a content-based identity for the ff binary: version string plus the
# first 12 hex characters of the SHA-256 of the binary's bytes. A compile-time
# git sha can go stale in an incremental build, and it says nothing about which
# build of a given commit (release and dogfood profiles produce different
# binaries from identical source). The bytes cannot lie.
ff_build_id() {
    local bin=$1
    local ver; ver=$("$bin" --version)
    local hash=""
    if command -v sha256sum > /dev/null 2>&1; then
        hash=$(sha256sum "$bin" | cut -c1-12)
    elif command -v shasum > /dev/null 2>&1; then
        hash=$(shasum -a 256 "$bin" | cut -c1-12)
    else
        # No hasher available: fall back to size+mtime. This is weaker than a
        # content hash -- two different binaries of the same size and mtime
        # would collide -- but it is better than the version string alone.
        hash=$(stat -c '%s%Y' "$bin" 2>/dev/null | md5sum | cut -c1-12) || \
        hash=$(stat -f '%z%m' "$bin" 2>/dev/null | md5sum | cut -c1-12) || \
        hash="nohash"
    fi
    echo "$ver $hash"
}

# The measured binary is the fat-LTO release build by default, never the
# incremental dogfood profile (Makefile:3, Cargo.toml's [profile.dogfood]
# comment) -- a dogfood binary would make every measurement here a lie about
# what a release actually costs. --ff-binary lets the operator deliberately
# measure a different build; using it for a dogfood binary makes the numbers
# a lie about release cost.
if [[ -n "$FF_BINARY_ARG" ]]; then
    # An explicitly named binary is the operator's choice; quietly building
    # over it would defeat the purpose of the flag.
    case "$FF_BINARY_ARG" in
        /*) FF="$FF_BINARY_ARG" ;;
        *) FF="$ROOT_DIR/$FF_BINARY_ARG" ;;
    esac
    if [[ ! -x "$FF" ]]; then
        echo "run.sh: --ff-binary path is not an executable file: $FF" >&2
        exit 2
    fi
else
    FF="$ROOT_DIR/target/release/ff"
    if [[ ! -x "$FF" ]]; then
        info "building release binary (target/release/ff missing)..."
        (cd "$ROOT_DIR" && cargo build --release -q)
    fi
fi

FF_BUILD_ID=$(ff_build_id "$FF")

# Hermetic git environment, lifted from scripts/bench.sh: no user config
# leaking in, fixed identity and timestamps so fixtures are reproducible.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=bench GIT_AUTHOR_EMAIL=bench@bench.test
export GIT_COMMITTER_NAME=bench GIT_COMMITTER_EMAIL=bench@bench.test
export GIT_AUTHOR_DATE="@1600000000 +0000" GIT_COMMITTER_DATE="@1600000000 +0000"

# jj: prefer PATH, fall back to the pinned ~/.local/bin/jj (0.44.0). CI has
# neither, so a missing jj drops it from TOOLS with a stderr note instead of
# failing the run -- git and ff still measure fine without it.
JJ_BIN=""
if [[ " ${TOOLS[*]} " == *" jj "* ]]; then
    if command -v jj > /dev/null 2>&1; then
        JJ_BIN=$(command -v jj)
    elif [[ -x "$HOME/.local/bin/jj" ]]; then
        JJ_BIN="$HOME/.local/bin/jj"
    fi
    if [[ -z "$JJ_BIN" ]]; then
        echo "run.sh: jj not found on PATH or ~/.local/bin/jj -- skipping jj rows" >&2
        NEW_TOOLS=()
        for _t in "${TOOLS[@]}"; do [[ "$_t" == "jj" ]] || NEW_TOOLS+=("$_t"); done
        TOOLS=("${NEW_TOOLS[@]}")
        unset _t NEW_TOOLS
    fi
fi

# Hermetic jj config, same reasoning as the git block above: fixed identity
# so fixture builds don't warn about an unset one, no color/pager surprises
# under hyperfine (which never runs these as a tty). JJ_CONFIG, if set,
# replaces file-based config entirely rather than layering on top of it, so
# this is enough on its own regardless of the operator's real config.
if [[ -n "$JJ_BIN" ]]; then
    mkdir -p "$FIXTURES_DIR"
    JJ_CONFIG_FILE="$FIXTURES_DIR/.jj-bench-config.toml"
    cat > "$JJ_CONFIG_FILE" <<'EOF'
[user]
name = "bench"
email = "bench@bench.test"
[ui]
color = "never"
paginate = "never"
EOF
    export JJ_CONFIG="$JJ_CONFIG_FILE"
fi

GENERATED_UNIX=$(date +%s)

resolve_binary() {
    case "$1" in
        ff) echo "$FF" ;;
        git) command -v git ;;
        jj) echo "$JJ_BIN" ;;
        *) echo "run.sh: unknown tool: $1" >&2; exit 2 ;;
    esac
}

# --- rows.tsv -------------------------------------------------------------
# Parallel arrays indexed together; bash has no records, and the table is a
# handful of rows long, so this is simpler than anything cleverer.
ROW_NAME=() ROW_AXIS=() ROW_EXPECT=() ROW_PREPARE=()
ROW_FF=() ROW_GIT=() ROW_JJ=()

read_rows() {
    local line
    while IFS=$'\t' read -r name axis expect prepare ffcol gitcol jjcol; do
        [[ -z "$name" || "$name" == \#* ]] && continue
        ROW_NAME+=("$name") ROW_AXIS+=("$axis") ROW_EXPECT+=("$expect") ROW_PREPARE+=("$prepare")
        ROW_FF+=("$ffcol") ROW_GIT+=("$gitcol") ROW_JJ+=("$jjcol")
    done < "$ROOT_DIR/scripts/bench/rows.tsv"
}
read_rows

row_col_for_tool() {
    local idx=$1 tool=$2
    case "$tool" in
        ff) echo "${ROW_FF[$idx]}" ;;
        git) echo "${ROW_GIT[$idx]}" ;;
        jj) echo "${ROW_JJ[$idx]}" ;;
    esac
}

row_selected() {
    # --rows filters by name only, so a name shared across axes (log) is
    # selected on every axis it appears in -- the axis loop already narrows
    # that separately.
    [[ -z "$ROWS_ARG" ]] && return 0
    local name=$1 want
    IFS=',' read -r -a wanted <<<"$ROWS_ARG"
    for want in "${wanted[@]}"; do
        [[ "$want" == "$name" ]] && return 0
    done
    return 1
}

# --- fixtures ---------------------------------------------------------------
# bench-fixtures/<axis>/<n>/{stamp-ff,stamp-git,stamp-jj,restore-id,
#   saved/{chain-tip,ids-live,git-base-sha,jj-base-opid},ff/,git/,jj/}
#
# ff/, git/ and jj/ are independent copies of the fixture at this point,
# never colocated with each other (Decision 1 in the plan brief): no tool's
# metadata ever ends up inside the tree another tool is measured in.
#
# cmd-*.sh and prepare-*.sh live at the fixture-point level, as siblings of
# ff/ (and git/, jj/), never inside them: restore-at's row runs `ff restore
# --all`, which wipes anything in the worktree that is not part of the
# target snapshot's tree -- untracked scratch files included. A cmd script
# sitting inside ff/ would delete itself out from under hyperfine on the
# very first timed run.
#
# real-history nests one level deeper --
# bench-fixtures/real-history/<repo-alias>/<n>/ -- so several pinned repos
# can be built and cached side by side without swapping --real-repo
# clobbering a fixture already built for a different one.

fixture_point_dir() { echo "$FIXTURES_DIR/$1/$2"; }

axis_dir_key() {
    if [[ "$1" == "real-history" ]]; then
        echo "real-history/$REAL_REPO_ALIAS"
    else
        echo "$1"
    fi
}

backdate_tree() {
    # Kill racy-clean: git (and ff, which walks the same stat cache through
    # gix) can only skip re-hashing an unchanged file when its mtime is far
    # enough from the index's cached mtime that a same-second coincidence
    # can't be mistaken for "definitely unchanged". Backdating before the
    # operation that writes the cache (git commit, or here the final ff
    # capture) is what makes the cache and the mtime agree on an old date.
    find . -path ./.git -prune -o -type f -print0 | xargs -0 touch -t 202001010000.00
}

seed_tree() {
    git init -q -b main
    git config user.name bench
    git config user.email bench@bench.test
    local i
    for i in $(seq 0 9); do echo "seed $i" > "f$i.txt"; done
    git add -A
    git commit -qm seed
}

build_ff_chain_depth() {
    local n=$1 dir=$2
    mkdir -p "$dir/ff"
    ( cd "$dir/ff" && seed_tree
      local i
      for ((i = 1; i <= n; i++)); do
          printf 'chain %d\n' "$i" > f0.txt
          "$FF" > /dev/null
          if ((i % 1000 == 0)); then info "  chain-depth/$n ff: $i/$n captures"; fi
      done
      # A dirty tracked file would make every capture-first read command
      # (status, capture, restore-at) append a snapshot mid-measurement, so
      # the fixture must land clean: back to the seed content, then one more
      # capture so the chain's own tip agrees with the worktree too.
      git checkout -q -- f0.txt
      backdate_tree
      "$FF" > /dev/null
    )
}

build_ff_history_depth() {
    local n=$1 dir=$2
    mkdir -p "$dir/ff"
    ( cd "$dir/ff" && seed_tree
      local i
      for ((i = 1; i <= n; i++)); do
          printf 'history %d\n' "$i" > f0.txt
          GIT_AUTHOR_DATE="@$((1600000000 + i * 60)) +0000" \
          GIT_COMMITTER_DATE="@$((1600000000 + i * 60)) +0000" \
              git commit -qam "history commit $i"
          if ((i % 1000 == 0)); then info "  history-depth/$n ff: $i/$n commits"; fi
      done
      # A bare capture on a tree that already matches HEAD exactly is a
      # no-op -- there is nothing to snapshot, so no chain gets created at
      # all. Dirty the file first to force one snapshot into existence, then
      # revert it and capture the now-clean state too, so both the chain
      # tip and HEAD agree and ff log interleaves against a real chain.
      printf 'history %d\n' "$((n + 1))" > f0.txt
      "$FF" > /dev/null
      git checkout -q -- f0.txt
      backdate_tree
      "$FF" > /dev/null
    )
}

# file-count spreads n small files across n/100 directories -- bench.sh's own
# 5000-file repo, generalized to any n. Kept as a plain loop rather than
# something cleverer: it only runs once per (axis, n) thanks to the stamp
# cache, and the 50000-file point prints its own progress below because a
# silent multi-minute loop there reads as a hang.
seed_file_count_tree() {
    local n=$1 i d
    for ((i = 0; i < n; i++)); do
        d=$((i / 100))
        mkdir -p "dir$d"
        echo "content $i" > "dir$d/f$((i % 100)).txt"
        if ((n >= 5000 && (i + 1) % 5000 == 0)); then info "  file-count/$n: $((i + 1))/$n files"; fi
    done
}

# Backdating happens before the first commit here, not after it the way
# build_ff_chain_depth's does: that builder's seed tree is ten tiny files, so
# a post-commit mtime/index mismatch is unmeasurable noise, but file-count's
# whole point is n real files, and a mismatch there would force git (and ff,
# through the same gix stat cache) to rehash the entire tree on every
# status-like row -- exactly the mass-rehashing backdate_tree exists to kill.
# Backdating before git add is what scripts/bench.sh did for the same reason.
build_ff_file_count() {
    local n=$1 dir=$2
    mkdir -p "$dir/ff"
    ( cd "$dir/ff" && git init -q -b main
      git config user.name bench
      git config user.email bench@bench.test
      seed_file_count_tree "$n"
      backdate_tree
      git add -A
      git commit -qm "file-count $n"
      # bench-other is switch's and park-arrive's destination branch -- only
      # ff has a switch row on this axis, so only ff's copy needs it.
      git branch bench-other
      local target; target=$(git ls-files | head -1)
      local i
      for i in 1 2 3; do
          printf 'history %d\n' "$i" >> "$target"
          "$FF" > /dev/null
      done
      # Same close-out as build_ff_chain_depth: revert the history commits'
      # dirtied file, backdate again (the loop above moved its mtime back to
      # "now"), and capture once more so ff's own cache and HEAD both agree
      # with the tree as it will actually be measured.
      git checkout -q -- "$target"
      backdate_tree
      "$FF" > /dev/null
    )
    build_fresh_copy ff "$dir"
}

# first-capture's `fresh` prepare deletes .git (and .jj) on every timed run
# by design, which would otherwise wipe the very chain-tip/git-base-sha/
# jj-base-opid the other rows on this fixture point depend on -- ff, git and
# jj each get their own untracked sibling copy for it instead of touching
# $dir/$tool directly, the same "never colocated" reasoning as ff/, git/ and
# jj/ themselves, just applied one row further. A plain cp -a plus stripping
# VCS metadata is far cheaper than reseeding n files from scratch on every
# fixture build, and correctness only needs the file *content* to match --
# first-capture's whole point is measured with no VCS metadata present yet.
build_fresh_copy() {
    local tool=$1 dir=$2
    rm -rf "$dir/fresh-$tool"
    cp -a "$dir/$tool" "$dir/fresh-$tool"
    rm -rf "$dir/fresh-$tool/.git" "$dir/fresh-$tool/.jj"
}

# git has no automatically-recorded working-tree state, so unlike ff's and
# jj's, its chain-depth copy never grows with n -- it is the same one-commit
# seed tree at every point, built (and measured) only at POINTS_MAX. This is
# what makes the "log" row's git column a single reference number rather
# than a scaling claim: report.py's single-point verdict, and the table
# showing dashes at every other n, already say so without needing a row-
# level notes field the frozen schema does not have.
build_git_chain_depth() {
    local dir=$1
    mkdir -p "$dir/git"
    ( cd "$dir/git" && seed_tree && backdate_tree )
}

# history-depth is plain commit count, the one axis that means the same
# thing to git as to the other two, so git gets a real growing copy here.
build_git_history_depth() {
    local n=$1 dir=$2
    mkdir -p "$dir/git"
    ( cd "$dir/git" && seed_tree
      local i
      for ((i = 1; i <= n; i++)); do
          printf 'history %d\n' "$i" > f0.txt
          GIT_AUTHOR_DATE="@$((1600000000 + i * 60)) +0000" \
          GIT_COMMITTER_DATE="@$((1600000000 + i * 60)) +0000" \
              git commit -qam "history commit $i"
          if ((i % 1000 == 0)); then info "  history-depth/$n git: $i/$n commits"; fi
      done
      backdate_tree
    )
}

# git's file-count copy, same backdate-before-the-big-commit ordering as
# build_ff_file_count above and for the same reason -- see that function's
# comment.
build_git_file_count() {
    local n=$1 dir=$2
    mkdir -p "$dir/git"
    ( cd "$dir/git" && git init -q -b main
      git config user.name bench
      git config user.email bench@bench.test
      seed_file_count_tree "$n"
      backdate_tree
      git add -A
      git commit -qm "file-count $n"
      local target; target=$(git ls-files | head -1)
      local i
      for i in 1 2 3; do
          printf 'history %d\n' "$i" >> "$target"
          GIT_AUTHOR_DATE="@$((1600000000 + i * 60)) +0000" \
          GIT_COMMITTER_DATE="@$((1600000000 + i * 60)) +0000" \
              git commit -qam "history commit $i"
      done
      backdate_tree
    )
    build_fresh_copy git "$dir"
}

# jj snapshots the working copy as a side effect of every command, so
# growing jj's own snapshot history to n is: rewrite the tracked file, run a
# command, n times -- that yields one change with n evolog entries and n+
# operations, the structural parallel to fufu's chain (never built by
# colocating over ff's or git's chain: that would be a different, dishonest
# axis -- see the plan brief's Decision 2). Budget ~15-30ms per jj
# invocation; the n=10000 point takes a few minutes the first time, hence
# the explicit "this can take a while" note where this is called.
build_jj_chain_depth() {
    local n=$1 dir=$2
    mkdir -p "$dir/jj"
    ( cd "$dir/jj" && "$JJ_BIN" git init . > /dev/null
      local i
      for ((i = 1; i <= n; i++)); do
          printf 'chain %d\n' "$i" > f0.txt
          "$JJ_BIN" status > /dev/null
          if ((i % 1000 == 0)); then info "  chain-depth/$n jj: $i/$n snapshots"; fi
      done
    )
}

# On history-depth, jj's copy is produced the way a user would actually get
# one: colocate jj onto an existing git repo. This depends on
# build_git_history_depth having already populated dir/git for this fixture
# point (ensure_fixture below orders that), and copies rather than colocates
# in place so dir/git itself -- what git is measured in -- never grows a
# .jj (Decision 1: no tool's metadata inside another tool's measured tree).
build_jj_history_depth() {
    local dir=$1
    rm -rf "$dir/jj"
    cp -a "$dir/git" "$dir/jj"
    ( cd "$dir/jj" && "$JJ_BIN" git init --colocate . > /dev/null )
}

# jj's file-count copy, same colocate-onto-git approach and the same
# reasoning as build_jj_history_depth above (and it depends on the same
# ordering: dir/git must already exist -- ensure_fixture below guarantees
# that for file-count the same way it does for history-depth).
build_jj_file_count() {
    local dir=$1
    rm -rf "$dir/jj"
    cp -a "$dir/git" "$dir/jj"
    ( cd "$dir/jj" && "$JJ_BIN" git init --colocate . > /dev/null )
    build_fresh_copy jj "$dir"
}

# real-history's fixture is a shallow clone of a pinned SHA at exactly depth
# n, built once into a staging dir and then copied out to ff/ and git/ --
# not built independently per tool the way the synthetic axes are, because
# the fetch itself is the expensive part (see the plan brief's cost note)
# and there is no reason to pay it twice for the same (repo, depth). jj's
# copy is made afterward from git/, same as build_jj_history_depth.
#
# Checking out onto a branch named main (never a detached HEAD) matters
# because the rest of this script -- refs/fufu/snap/main, the `reset`
# prepare, the saved id index -- already assumes that branch name; fetching
# an arbitrary pinned SHA works at all because GitHub enables
# uploadpack.allowAnySHA1InWant.
#
# The base is fetched clean, with no ff capture inside it. The old code
# captured in the staging dir before copying, so $dir/git and (via it)
# $dir/jj both carried refs/fufu/* and .git/fufu/ — fufu metadata inside
# the trees other tools are measured in, violating the invariant at
# run.sh:262. After the split only $dir/ff is ever captured into.
build_real_history_base_git() {
    local n=$1 dir=$2 stage="$dir/.stage"
    rm -rf "$stage"
    mkdir -p "$stage"
    if ! ( cd "$stage" \
        && git init -q -b main \
        && git remote add origin "$REAL_REPO_URL" \
        && git fetch -q --depth "$n" origin "$REAL_REPO_SHA" \
        && git checkout -q -b main FETCH_HEAD ); then
        rm -rf "$stage"
        return 1
    fi
    ( cd "$stage"
      git config user.name bench
      git config user.email bench@bench.test
      backdate_tree
    )
    # What actually got fetched, which is not what was asked for: results are
    # reported against this count, never against the requested depth. Labeling
    # a row "n=500" when the repo behind it holds 81k commits would misstate
    # the scale by two orders of magnitude.
    ( cd "$stage" && git rev-list --count HEAD ) > "$dir/commits"
    # mv rather than cp -a: it is a rename on the same filesystem and saves
    # copying gigabytes.
    mv "$stage" "$dir/git"
}

# $dir/ff is derived from the pristine $dir/git. The ff captures that force
# a snapshot chain into existence happen here, not in the base, so the git
# and jj copies never see fufu metadata.
#
# The order of steps is load-bearing: step 1 dirties the first tracked file
# and sets its mtime to now; step 2 (backdate_tree) puts it back to 2020 so
# the racy-clean logic at run.sh:286-294 behaves; step 3 writes the stat
# cache against those backdated mtimes. Swapping 1 and 2 leaves one file
# with a fresh mtime and makes `ff status` on the "clean" fixture do real
# work.
build_real_history_base_ff() {
    local dir=$1
    rm -rf "$dir/ff"
    cp -a "$dir/git" "$dir/ff"
    ( cd "$dir/ff"
      # 1. dirty the first tracked file, capture, revert
      local dirty_file; dirty_file=$(git ls-files | head -1)
      if [[ -n "$dirty_file" ]]; then
          printf '\nbench dirty\n' >> "$dirty_file"
          "$FF" > /dev/null
          git checkout -q -- "$dirty_file"
      fi
      # 2. backdate so racy-clean doesn't fire
      backdate_tree
      # 3. capture the clean state against backdated mtimes
      "$FF" > /dev/null
    )
}

# The n a result is reported under. On the synthetic axes the requested point
# is the truth -- the builder made exactly that many snapshots or commits. On
# real-history the fetch decides, so the measured commit count stands in.
emitted_n() {
    local axis=$1 n=$2 dir count
    if [[ "$axis" == "real-history" ]]; then
        dir=$(fixture_point_dir "$(axis_dir_key "$axis")" "$n")
        # Counted from the fixture rather than read from a file the builder
        # wrote, so a fixture built before this file existed still reports
        # honestly instead of quietly falling back to the requested depth.
        if [[ ! -s "$dir/commits" && -d "$dir/ff" ]]; then
            count=$(git -C "$dir/ff" rev-list --count HEAD 2>/dev/null) || count=""
            [[ -n "$count" ]] && printf '%s\n' "$count" > "$dir/commits"
        fi
        if [[ -s "$dir/commits" ]]; then
            cat "$dir/commits"
            return
        fi
    fi
    echo "$n"
}

# saved/chain-tip, saved/ids-live and restore-id all describe ff's fixture
# steady state so `reset` prepare (below) can return to it in O(1), and so
# {ID} substitution has a real snapshot id to hand restore-at.
record_ff_fixture_state() {
    local dir=$1
    mkdir -p "$dir/saved"

    # Capture chain-tip, settle the id index, and query the snapshot log.
    # Settle the id index before saving it. Building a fixture by n
    # sequential captures leaves an index of one sorted record plus an n-long
    # unsorted tail, and the first read past MERGE_TAIL pays a one-time O(n)
    # merge to compact it -- amortized to nothing in real use, where it
    # happens once. Saving the pre-merge file would make `reset` restore that
    # debt before every timed run, so every run would pay a cost a user pays
    # once, and capture-ish rows would read as O(chain) when they are not.
    # CLICOLOR_FORCE because the prefix table -- and so the index read that
    # triggers the merge -- is skipped entirely when color is off.
    if ! ( cd "$dir/ff"
      git rev-parse refs/fufu/snap/main > "../saved/chain-tip"
      CLICOLOR_FORCE=1 "$FF" evolog -n 1 > /dev/null 2>&1 || true
      # May not exist on a one-snapshot chain -- record() still writes it
      # unconditionally on this codebase, but tolerate absence rather than
      # assume that stays true.
      if [[ -f .git/fufu/ids/live/main ]]; then
          cp .git/fufu/ids/live/main "../saved/ids-live"
      else
          rm -f "../saved/ids-live"
      fi
      # `op log`, not `evolog`: {ID} feeds --at-op, which is an
      # operation-typed position and takes letters only. `ff op log --json`
      # is the one view whose ids are already in that spelling, so nothing
      # here has to re-implement the alphabet.
      "$FF" op log -n 0 --json > "../.oplog.json"
    ); then
        echo "run.sh: fixture build failed — '$FF' exited non-zero during op log; this ff build cannot construct bench fixtures (op log --json is required, usually missing on binaries predating that command)" >&2
        exit 1
    fi

    # Parse the op log JSON to extract the mid-log operation id for restore-at.
    # A JSONDecodeError here means op log produced no valid output — same root
    # cause as above (binary too old or broken), caught explicitly rather than
    # letting python3 traceback be the error signal.
    python3 - "$dir" <<'PY' || { echo "run.sh: fixture build failed — '$FF' op log --json output was not valid JSON; this ff build cannot construct bench fixtures (op log --json is required, usually missing on binaries predating that command)" >&2; exit 1; }
import json, sys
d = sys.argv[1]
with open(f"{d}/.oplog.json") as f:
    data = json.load(f)
ids = [s["id"] for s in data["data"]["ops"]]
mid = ids[len(ids) // 2] if ids else ""
with open(f"{d}/restore-id", "w") as f:
    f.write(mid)
PY
    rm -f "$dir/.oplog.json"

    # Verify the artifacts we depend on for measurement exist.
    # saved/chain-tip is what reset prepare restores; restore-id is the {ID}
    # substituted into restore-at rows. Without either, every measured row
    # reports numbers that mean nothing — the exact false-linear-term trap
    # this suite was designed to catch.
    # Note: --keep-going guards the per-row smoke run, not fixture
    # construction. A fixture that did not build corrupts every row measured
    # against it, so there is nothing to "keep going" past.
    if [[ ! -s "$dir/saved/chain-tip" ]]; then
        echo "run.sh: fixture build failed — saved/chain-tip is missing or empty for '$FF'; this ff build cannot construct bench fixtures (op log --json is required, usually missing on binaries predating that command)" >&2
        exit 1
    fi
    if [[ ! -s "$dir/restore-id" ]]; then
        echo "run.sh: fixture build failed — restore-id is missing or empty for '$FF'; this ff build cannot construct bench fixtures (op log --json is required, usually missing on binaries predating that command)" >&2
        exit 1
    fi
}

# saved/git-base-sha is git's steady state, used the same way as ff's
# chain-tip: `git reset --hard` back to it is git's O(1) equivalent, so a
# measured `capture` row does not let git's commit count drift across
# warmup and timed runs the way an un-reset chain would.
record_git_fixture_state() {
    local dir=$1
    mkdir -p "$dir/saved"
    ( cd "$dir/git" && git rev-parse HEAD ) > "$dir/saved/git-base-sha"
}

# saved/jj-base-opid is jj's steady state, same role as the two above:
# `jj op restore` back to this operation is jj's O(1) reset, undoing the
# extra snapshot a measured `capture` row (jj status on a dirtied file)
# would otherwise leave behind.
record_jj_fixture_state() {
    local dir=$1
    mkdir -p "$dir/saved"
    ( cd "$dir/jj" && "$JJ_BIN" op log --no-graph -n 1 -T 'self.id()' ) > "$dir/saved/jj-base-opid"
}

# One stamp file per tool rather than one per fixture point: ff, git and jj
# are built and cached independently, so a --tools ff run must never
# rebuild git or jj fixtures (or vice versa) just because it didn't ask for
# them last time.
tool_fixture_fresh() {
    local dir=$1 tool=$2 want=$3
    [[ -f "$dir/stamp-$tool" ]] && [[ "$(cat "$dir/stamp-$tool")" == "$want" ]]
}

# real-history degrades rather than breaks: a clone failure (offline, GitHub
# down, a timed-out fetch) prints a message naming the URL and returns
# non-zero so the caller can skip the axis, instead of exit-ing the whole
# run the way a hard failure on the hermetic axes does. It is never run in
# CI and never gates, so there is nothing here worth failing a build over.
#
# jj may also refuse a shallow git repo outright -- checked empirically here
# rather than assumed. If it refuses, this leaves no dir/jj behind and prints
# a note; the main loop below skips jj rows for this axis when dir/jj is
# absent, i.e. null jj results rather than a failed run. Never worked around
# by deepening the clone -- that would silently change what the axis means.
ensure_real_history_fixture() {
    local n=$1 dir=$2

    local want_ff="$FIXTURE_STAMP_FORMAT real-history $n $REAL_REPO_ALIAS $REAL_REPO_URL $REAL_REPO_SHA $FF_BUILD_ID"
    local want_git="$FIXTURE_STAMP_FORMAT real-history $n $REAL_REPO_ALIAS $REAL_REPO_URL $REAL_REPO_SHA git"
    local need_git=0 need_ff=0
    tool_fixture_fresh "$dir" git "$want_git" || need_git=1
    tool_fixture_fresh "$dir" ff "$want_ff" || need_ff=1
    [[ -d "$dir/git" ]] || need_git=1

    # If the git base is stale, fetch and then necessarily re-derive ff from
    # the fresh clone. If only ff is stale, re-derive ff alone — cp -a plus
    # three ff invocations, no network. A stale git base always implies a
    # stale ff copy (the ff copy comes from git), so clear need_ff to avoid
    # duplicating the derive step.
    if [[ $need_git -eq 1 ]]; then
        info "fixture real-history/$REAL_REPO_ALIAS/$n: fetching $REAL_REPO_URL @ $REAL_REPO_SHA depth $n (this can take minutes at large n, not a hang)..."
        if ! build_real_history_base_git "$n" "$dir"; then
            echo "run.sh: fetch of $REAL_REPO_URL (depth $n) failed -- real-history needs network access to GitHub; skipping the axis" >&2
            return 1
        fi
        record_git_fixture_state "$dir"
        echo "$want_git" > "$dir/stamp-git"
        need_ff=1
    fi

    if [[ $need_ff -eq 1 ]]; then
        info "fixture real-history/$REAL_REPO_ALIAS/$n: deriving ff copy from git base..."
        build_real_history_base_ff "$dir"
        record_ff_fixture_state "$dir"
        echo "$want_ff" > "$dir/stamp-ff"
        local size; size=$(du -sh "$dir/git" 2>/dev/null | cut -f1)
        info "fixture real-history/$REAL_REPO_ALIAS/$n: built (${size:-?} on disk per tool copy)"
    else
        info "fixture real-history/$REAL_REPO_ALIAS/$n: reused"
    fi

    if [[ -n "$JJ_BIN" ]] && [[ " ${TOOLS[*]} " == *" jj "* ]]; then
        local jj_version; jj_version=$("$JJ_BIN" --version)
        local want_jj="$FIXTURE_STAMP_FORMAT real-history $n $REAL_REPO_ALIAS $REAL_REPO_URL $REAL_REPO_SHA $jj_version"
        if tool_fixture_fresh "$dir" jj "$want_jj" && [[ -d "$dir/jj" ]]; then
            info "fixture real-history/$REAL_REPO_ALIAS/$n jj: reused"
        else
            rm -rf "$dir/jj"
            cp -a "$dir/git" "$dir/jj"
            local jj_err="$dir/.jj-init-err"
            if ( cd "$dir/jj" && "$JJ_BIN" git init --colocate . > /dev/null 2>"$jj_err" ); then
                record_jj_fixture_state "$dir"
                echo "$want_jj" > "$dir/stamp-jj"
                info "fixture real-history/$REAL_REPO_ALIAS/$n jj: built"
            else
                info "fixture real-history/$REAL_REPO_ALIAS/$n jj: refused a shallow clone ($(tail -1 "$jj_err" 2>/dev/null)) -- skipping jj on this axis, null jj results"
                rm -rf "$dir/jj"
                rm -f "$dir/stamp-jj"
            fi
            rm -f "$jj_err"
        fi
    fi

    return 0
}

ensure_fixture() {
    local axis=$1 n=$2
    local dir; dir=$(fixture_point_dir "$(axis_dir_key "$axis")" "$n")
    mkdir -p "$dir"

    if [[ "$axis" == "real-history" ]]; then
        ensure_real_history_fixture "$n" "$dir"
        return
    fi

    # The ff stamp carries FF_BUILD_ID (version + content hash of the binary)
    # instead of the version string alone. Any change to the ff binary now
    # rebuilds ff fixtures. That is the point — a change to what ff writes at
    # capture time must not be measured against a chain in the old format.
    # The cost is a cold chain-depth 10 000 rebuild of roughly 40 s.
    local want_ff="$FIXTURE_STAMP_FORMAT $axis $n $FF_BUILD_ID"
    if tool_fixture_fresh "$dir" ff "$want_ff"; then
        info "fixture $axis/$n ff: reused"
    else
        info "fixture $axis/$n ff: building..."
        rm -rf "$dir/ff"
        case "$axis" in
            chain-depth) build_ff_chain_depth "$n" "$dir" ;;
            history-depth) build_ff_history_depth "$n" "$dir" ;;
            file-count) build_ff_file_count "$n" "$dir" ;;
        esac
        record_ff_fixture_state "$dir"
        echo "$want_ff" > "$dir/stamp-ff"
        info "fixture $axis/$n ff: built"
    fi

    # git's chain-depth copy is only ever needed at POINTS_MAX (see
    # POINTS_MAX above and build_git_chain_depth); on history-depth and
    # file-count it is needed at every measured point, and also whenever jj
    # is selected, since jj's history-depth and file-count copies both
    # colocate onto git's.
    local want_git=0
    [[ " ${TOOLS[*]} " == *" git "* ]] && want_git=1
    [[ ( "$axis" == "history-depth" || "$axis" == "file-count" ) && " ${TOOLS[*]} " == *" jj "* ]] && want_git=1
    if [[ $want_git -eq 1 ]] && [[ "$axis" == "history-depth" || "$axis" == "file-count" || "$n" == "$POINTS_MAX" ]]; then
        local git_version; git_version=$(git --version)
        local want="$FIXTURE_STAMP_FORMAT $axis $n $git_version"
        if tool_fixture_fresh "$dir" git "$want"; then
            info "fixture $axis/$n git: reused"
        else
            info "fixture $axis/$n git: building..."
            rm -rf "$dir/git"
            case "$axis" in
                chain-depth) build_git_chain_depth "$dir" ;;
                history-depth) build_git_history_depth "$n" "$dir" ;;
                file-count) build_git_file_count "$n" "$dir" ;;
            esac
            record_git_fixture_state "$dir"
            echo "$want" > "$dir/stamp-git"
            info "fixture $axis/$n git: built"
        fi
    fi

    if [[ -n "$JJ_BIN" ]] && [[ " ${TOOLS[*]} " == *" jj "* ]]; then
        local jj_version; jj_version=$("$JJ_BIN" --version)
        local want="$FIXTURE_STAMP_FORMAT $axis $n $jj_version"
        if tool_fixture_fresh "$dir" jj "$want"; then
            info "fixture $axis/$n jj: reused"
        else
            info "fixture $axis/$n jj: building (a several-minute cold build at large n is expected here, not a hang)..."
            rm -rf "$dir/jj"
            case "$axis" in
                chain-depth) build_jj_chain_depth "$n" "$dir" ;;
                history-depth) build_jj_history_depth "$dir" ;;
                file-count) build_jj_file_count "$dir" ;;
            esac
            record_jj_fixture_state "$dir"
            echo "$want" > "$dir/stamp-jj"
            info "fixture $axis/$n jj: built"
        fi
    fi
}

# --- prepare / cmd scripts ---------------------------------------------------
# Regenerated every run regardless of fixture caching: they are cheap, and
# they depend on which rows/tools/points this invocation selected, which can
# change run to run even when the underlying fixture repo does not.

gen_prepare() {
    local dir=$1 tool=$2 kind=$3
    local path="$dir/prepare-$kind-$tool.sh"
    case "$kind" in
        -) return ;;
        dirty)
            # Resolved from the fixture rather than hardcoded: the synthetic
            # builders make an f0.txt, but a real repo has no such file, and
            # writing one there would measure untracked-file handling under
            # the name "modified file" -- and leave the file behind in a
            # cached fixture, so the next run's clean rows would not be clean.
            local target; target=$(git -C "$dir/$tool" ls-files 2>/dev/null | head -1)
            [[ -n "$target" ]] || target=f0.txt
            cat > "$path" <<EOF
cd '$dir/$tool'
printf 'dirty %s\n' "\$RANDOM" > '$target'
EOF
            ;;
        reset)
            # Each tool's own O(1) undo for the state a measured capture-ish
            # row (git commit, jj status, ff's bare capture) would otherwise
            # leave behind -- ff via its chain ref + id index, git via
            # reset --hard, jj via op restore. Without this, n drifts
            # upward across warmup and timed runs alike (see the header
            # comment in rows.tsv).
            local target; target=$(git -C "$dir/$tool" ls-files 2>/dev/null | head -1)
            [[ -n "$target" ]] || target=f0.txt
            case "$tool" in
                ff)
                    cat > "$path" <<EOF
cd '$dir/$tool'
git update-ref refs/fufu/snap/main "\$(cat '$dir/saved/chain-tip')"
if [[ -f '$dir/saved/ids-live' ]]; then
    mkdir -p .git/fufu/ids/live
    cp '$dir/saved/ids-live' .git/fufu/ids/live/main
fi
printf 'dirty %s\n' "\$RANDOM" > '$target'
EOF
                    ;;
                git)
                    cat > "$path" <<EOF
cd '$dir/$tool'
git reset -q --hard "\$(cat '$dir/saved/git-base-sha')"
printf 'dirty %s\n' "\$RANDOM" > '$target'
EOF
                    ;;
                jj)
                    cat > "$path" <<EOF
cd '$dir/$tool'
'$JJ_BIN' op restore "\$(cat '$dir/saved/jj-base-opid')" > /dev/null
printf 'dirty %s\n' "\$RANDOM" > '$target'
EOF
                    ;;
            esac
            ;;
        fresh)
            # The one prepare kind that is not O(1): rm -rf .git (and .jj,
            # for the jj column) scales with the number of git objects,
            # which scales with n. That is fine here -- hyperfine never
            # times --prepare, and first-capture's whole point is the cost
            # of a first capture, which cannot be measured repeatedly
            # without restoring "never captured" between every run.
            #
            # Runs against $dir/fresh-$tool, never $dir/$tool: this prepare
            # deletes .git (and .jj) on every timed run by design, and
            # $dir/$tool is the fixture every other row on this axis/n
            # shares -- see build_fresh_copy.
            case "$tool" in
                ff|git)
                    cat > "$path" <<EOF
cd '$dir/fresh-$tool'
rm -rf .git
git init -q -b main
git config user.name bench
git config user.email bench@bench.test
EOF
                    ;;
                jj)
                    cat > "$path" <<EOF
cd '$dir/fresh-jj'
rm -rf .git .jj
'$JJ_BIN' git init . > /dev/null
EOF
                    ;;
            esac
            ;;
    esac
    echo "$path"
}

gen_cmd() {
    local dir=$1 tool=$2 name=$3 binary=$4 args=$5 prepare_kind=$6
    local path="$dir/cmd-$name-$tool.sh"
    # first-capture's fresh prepare runs against $dir/fresh-$tool, not
    # $dir/$tool (see gen_prepare's fresh case and build_fresh_copy) -- the
    # measured command has to land in the same directory the prepare just
    # reset, or it would run against the wrong tree entirely.
    local target_dir="$dir/$tool"
    [[ "$prepare_kind" == "fresh" ]] && target_dir="$dir/fresh-$tool"
    # ff decides whether to compute the unique-prefix table by asking whether
    # color is on (cmd/evolog.rs: displayed_prefix_lens), and hyperfine sends
    # stdout to a pipe, so an unforced run skips that path entirely -- the id
    # index, the whole point of the snapshot-id work, would never be exercised
    # by a benchmark built to protect it. anstream honors CLICOLOR_FORCE, so
    # ff's rows measure the interactive path. It costs ff a fraction of a
    # millisecond of ANSI rendering that git and jj do not pay here; that
    # asymmetry runs against us, which is the right direction for it to run.
    : > "$path"
    [[ "$tool" == "ff" ]] && echo "export CLICOLOR_FORCE=1" >> "$path"
    if [[ "$args" == *" && "* ]]; then
        # exec would replace the shell before the second half of a compound
        # command ever ran, so a row whose argv is itself a shell list
        # (git's capture row: "add -A && commit -m x") gets a plain
        # sequential invocation instead of the usual exec-tail-call form.
        # Each &&-separated segment needs its own copy of the binary --
        # only the first one comes with it built in from rows.tsv -- so
        # this splits on the literal " && " rather than treating the
        # whole args string as a single invocation's argv.
        printf "cd '%s'\n" "$target_dir" >> "$path"
        local rest="$args" seg
        while [[ "$rest" == *" && "* ]]; do
            seg=${rest%% && *}
            rest=${rest#*" && "}
            printf "'%s' %s\n" "$binary" "$seg" >> "$path"
        done
        printf "exec '%s' %s\n" "$binary" "$rest" >> "$path"
    else
        printf "cd '%s' && exec '%s' %s\n" "$target_dir" "$binary" "$args" >> "$path"
    fi
    echo "$path"
}

gen_floor_cmd() {
    local dir=$1 tool=$2 binary=$3
    local path="$dir/cmd-floor-$tool.sh"
    printf "cd '%s/%s' && exec '%s' --version\n" "$dir" "$tool" "$binary" > "$path"
    echo "$path"
}

# Fixtures are cached and reused across rows and across whole runs, so
# whatever a row leaves behind becomes the next row's starting state: the
# modified file a `dirty` prepare wrote, the snapshot a capture row took. Left
# alone, a "clean tree" row measures a clean tree exactly once -- on the run
# that built the fixture -- and quietly measures a dirty one forever after.
# Every row starts from the fixture's recorded steady state instead. O(1) for
# ff and jj; git's sweep is O(files), and untimed.
restore_steady_state() {
    local dir=$1 tool=$2 target
    [[ -d "$dir/$tool" ]] || return 0
    case "$tool" in
        ff|git)
            git -C "$dir/$tool" checkout -q -- . 2>/dev/null || true
            git -C "$dir/$tool" clean -qfd > /dev/null 2>&1 || true
            ;;&
        ff)
            if [[ -s "$dir/saved/chain-tip" ]]; then
                git -C "$dir/$tool" update-ref refs/fufu/snap/main "$(cat "$dir/saved/chain-tip")"
            fi
            if [[ -f "$dir/saved/ids-live" ]]; then
                mkdir -p "$dir/$tool/.git/fufu/ids/live"
                cp "$dir/saved/ids-live" "$dir/$tool/.git/fufu/ids/live/main"
            fi
            ;;
        git)
            if [[ -s "$dir/saved/git-base-sha" ]]; then
                git -C "$dir/$tool" reset -q --hard "$(cat "$dir/saved/git-base-sha")"
            fi
            ;;
        jj)
            if [[ -s "$dir/saved/jj-base-opid" ]]; then
                "$JJ_BIN" -R "$dir/$tool" op restore "$(cat "$dir/saved/jj-base-opid")" \
                    > /dev/null 2>&1 || true
            fi
            ;;
    esac
    # Restoring a file gives it a fresh mtime, which puts it back in the racy
    # zone and costs a rehash on every later scan. Only the prepare target is
    # ever restored, so backdating that one path is enough to keep the tree in
    # the same lstat-compare regime backdate_tree established at build time.
    target=$(git -C "$dir/$tool" ls-files 2>/dev/null | head -1) || target=""
    [[ -n "$target" && -f "$dir/$tool/$target" ]] && touch -t 202001010000.00 "$dir/$tool/$target"
    return 0
}

# --- measurement --------------------------------------------------------

manifest_add() {
    python3 - "$@" >> "$MANIFEST" <<'PY'
import json, sys
kind, row, axis, expect, tool, n, prepare, command, export_json = sys.argv[1:10]
print(json.dumps({
    "kind": kind, "row": row, "axis": axis, "expect": expect, "tool": tool,
    "n": int(n), "prepare": prepare, "command": command, "export_json": export_json,
}))
PY
}

measure() {
    # kind: row|floor. name: the row's declared name, or "floor".
    local kind=$1 name=$2 axis=$3 expect=$4 tool=$5 n=$6 prepare=$7 human_cmd=$8 cmd_script=$9 prepare_script=${10}
    local dir; dir=$(fixture_point_dir "$axis" "$n")
    local export_json="$RAW_DIR/$axis-$n-$name-$tool.json"

    local hf_args=(--shell=none --warmup 3 --min-runs "$MIN_RUNS" --max-runs 50 -i --export-json "$export_json")
    if [[ -n "$prepare_script" ]]; then
        hf_args+=(--prepare "bash '$prepare_script'")
    fi

    # An untimed smoke run first, same shape as scripts/bench.sh's run_rows:
    # it catches a genuinely broken command before hyperfine spends warmup
    # time on it, and gives a clear "which command failed" message instead
    # of hyperfine's generic non-zero-exit complaint.
    local smoke_ok=1
    if [[ -n "$prepare_script" ]]; then bash "$prepare_script" > /dev/null 2>&1 || smoke_ok=0; fi
    if [[ $smoke_ok -eq 1 ]]; then bash "$cmd_script" > /dev/null 2>&1 || smoke_ok=0; fi

    if [[ $smoke_ok -eq 0 ]]; then
        if [[ $KEEP_GOING -eq 0 ]]; then
            echo "run.sh: command failed on smoke run: $human_cmd ($axis/$n, $tool)" >&2
            exit 1
        fi
        info "smoke run failed, continuing (--keep-going): $human_cmd ($axis/$n, $tool)"
    fi

    hyperfine "${hf_args[@]}" "bash '$cmd_script'" >&2

    local out_n; out_n=$(emitted_n "$axis" "$n")
    manifest_add "$kind" "$name" "$axis" "$expect" "$tool" "$out_n" "$prepare" "$human_cmd" "$export_json"

    local mean; mean=$(python3 -c "import json; print(json.load(open('$export_json'))['results'][0]['mean']*1000)")
    info "  $kind $name/$tool @$n: ${mean} ms mean"
}

# --- main -----------------------------------------------------------------

# jj snapshots the working copy at the start of every command, and ff is
# capture-first the same way, so both tools' "read" rows here (log, status,
# evolog, oplog) include the cost of recording the tree while git's do not
# -- git only pays that on an explicit add/commit. That is a real
# difference between the products, not a measurement flaw, but it is worth
# writing down here because it means ff's and jj's numbers on those rows
# are not directly comparable to git's even where all three have a column.

for axis in "${AXES[@]}"; do
    for n in "${POINTS[@]}"; do
        if ! ensure_fixture "$axis" "$n"; then
            # Only real-history degrades this way (see
            # ensure_real_history_fixture) -- any other axis failing here is
            # a real bug, not a network hiccup, and still stops the run.
            if [[ "$axis" == "real-history" ]]; then
                info "run.sh: real-history axis unavailable this run -- skipping remaining points"
                break
            fi
            exit 1
        fi
        dir=$(fixture_point_dir "$(axis_dir_key "$axis")" "$n")
        restore_id=""
        [[ -f "$dir/restore-id" ]] && restore_id=$(cat "$dir/restore-id")

        produced_tools=()

        for ((i = 0; i < ${#ROW_NAME[@]}; i++)); do
            [[ "${ROW_AXIS[$i]}" == "$axis" ]] || continue
            row_selected "${ROW_NAME[$i]}" || continue

            for tool in "${TOOLS[@]}"; do
                col=$(row_col_for_tool "$i" "$tool")
                [[ "$col" == "-" ]] && continue

                # git has nothing that grows on chain-depth (see POINTS_MAX
                # above): every git column on this axis is measured once,
                # at the largest point, as a reference cost rather than a
                # scaling claim.
                if [[ "$axis" == "chain-depth" && "$tool" == "git" && "$n" != "$POINTS_MAX" ]]; then
                    continue
                fi

                # jj may have refused a shallow real-history clone (see
                # ensure_real_history_fixture); no dir/jj means null jj
                # results for this axis, not a failed run.
                if [[ "$axis" == "real-history" && "$tool" == "jj" && ! -d "$dir/jj" ]]; then
                    continue
                fi

                args="$col"
                [[ "$args" == "(bare)" ]] && args=""
                args=${args//\{ID\}/$restore_id}

                binary=$(resolve_binary "$tool")
                binary_name=$(basename "$binary")
                human_cmd="$binary_name"
                [[ -n "$args" ]] && human_cmd="$binary_name $args"

                prepare_kind="${ROW_PREPARE[$i]}"
                prepare_script=""
                if [[ "$prepare_kind" != "-" ]]; then
                    prepare_script=$(gen_prepare "$dir" "$tool" "$prepare_kind")
                fi
                cmd_script=$(gen_cmd "$dir" "$tool" "${ROW_NAME[$i]}" "$binary" "$args" "$prepare_kind")

                # fresh's own --prepare script already deletes .git (and
                # .jj) and leaves the tree fully untracked by design --
                # restore_steady_state's `git clean -fd` would otherwise
                # delete the entire fixture tree, at every point, on a row
                # whose prepare is fresh. Skip it there; every other
                # prepare kind still gets the usual steady-state reset.
                if [[ "$prepare_kind" != "fresh" ]]; then
                    restore_steady_state "$dir" "$tool"
                fi

                measure row "${ROW_NAME[$i]}" "$axis" "${ROW_EXPECT[$i]}" "$tool" "$n" \
                    "$prepare_kind" "$human_cmd" "$cmd_script" "$prepare_script"

                if [[ ! " ${produced_tools[*]-} " == *" $tool "* ]]; then
                    produced_tools+=("$tool")
                fi
            done
        done

        # Exactly one floor per (tool, axis, n) that produced any row --
        # process startup dominates at these sizes, and report.py needs this
        # to subtract it back out before it can see the linear term at all.
        for tool in "${produced_tools[@]}"; do
            binary=$(resolve_binary "$tool")
            binary_name=$(basename "$binary")
            floor_cmd_script=$(gen_floor_cmd "$dir" "$tool" "$binary")
            measure floor floor "$axis" flat "$tool" "$n" "-" "$binary_name --version" "$floor_cmd_script" ""
        done
    done
done

# --- assemble bench-results/raw.json --------------------------------------

ff_version=$("$FF" --version)
git_version=null
jj_version=null
for tool in "${TOOLS[@]}"; do
    case "$tool" in
        git) git_version=$(git --version) ;;
        jj) jj_version=$("$JJ_BIN" --version) ;;
    esac
done
hyperfine_version=$(hyperfine --version)

axes_json="{"
first=1
for axis in "${AXES[@]}"; do
    [[ $first -eq 1 ]] || axes_json+=","
    first=0
    pts=$(printf '%s,' "${POINTS[@]}")
    pts="[${pts%,}]"
    if [[ "$axis" == "real-history" ]]; then
        # Additive over the frozen schema (bench-schema.md): repo provenance
        # lets a reader reproduce the exact fixture this axis measured.
        # points are the commit counts results are reported under; depths are
        # what was asked of git fetch. They differ, often wildly -- see
        # emitted_n -- and printing both is what makes that visible instead of
        # looking like a bug in the numbers.
        real_pts=$(for p in "${POINTS[@]}"; do printf '%s,' "$(emitted_n "$axis" "$p")"; done)
        real_pts="[${real_pts%,}]"
        axes_json+="\"$axis\":{\"points\":$real_pts,\"repo_alias\":\"$REAL_REPO_ALIAS\",\"repo_url\":\"$REAL_REPO_URL\",\"repo_sha\":\"$REAL_REPO_SHA\",\"depths\":$pts}"
    else
        axes_json+="\"$axis\":{\"points\":$pts}"
    fi
done
axes_json+="}"

python3 - "$MANIFEST" "$OUT_PATH" "$GENERATED_UNIX" "$ff_version" "$git_version" "$jj_version" \
    "$hyperfine_version" "$FF" "$MIN_RUNS" "$axes_json" "$FF_BUILD_ID" <<'PY'
import json, os, subprocess, sys

(manifest_path, out_path, generated_unix, ff_version, git_version, jj_version,
 hyperfine_version, ff_binary, min_runs, axes_json, ff_build_id) = sys.argv[1:12]


def null_or(v):
    return None if v == "null" else v


def cpu_model():
    try:
        out = subprocess.run(["lscpu"], capture_output=True, text=True, timeout=5)
        for line in out.stdout.splitlines():
            if line.strip().lower().startswith("model name"):
                return line.split(":", 1)[1].strip()
    except Exception:
        pass
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.lower().startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return None


uname = os.uname()
meta = {
    "generated_unix": int(generated_unix),
    "host": {
        "os": uname.sysname.lower(),
        "arch": uname.machine,
        "kernel": uname.release,
        "cpu": cpu_model(),
        "nproc": os.cpu_count(),
    },
    "versions": {
        "ff": ff_version,
        "git": null_or(git_version),
        "jj": null_or(jj_version),
        "hyperfine": hyperfine_version,
    },
    "ff_build_id": ff_build_id,
    "ff_binary": ff_binary,
    "axes": json.loads(axes_json),
    "flat_ratio_max": 1.5,
    "hyperfine": {"warmup": 3, "min_runs": int(min_runs), "max_runs": 50},
}

results = []
with open(manifest_path) as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        entry = json.loads(line)
        with open(entry.pop("export_json")) as ef:
            hf = json.load(ef)["results"][0]
        stddev = hf["stddev"]
        results.append({
            **entry,
            "mean_ms": hf["mean"] * 1000,
            "stddev_ms": (stddev * 1000) if stddev is not None else 0.0,
            "median_ms": hf["median"] * 1000,
            "min_ms": hf["min"] * 1000,
            "max_ms": hf["max"] * 1000,
            "runs": len(hf["times"]),
            "exit_ok": all(code == 0 for code in hf["exit_codes"]),
        })

with open(out_path, "w") as f:
    json.dump({"meta": meta, "results": results}, f, indent=2)
    f.write("\n")

print(f"wrote {out_path}: {len(results)} result rows", file=sys.stderr)
PY
