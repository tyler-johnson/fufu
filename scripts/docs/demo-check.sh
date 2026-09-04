#!/usr/bin/env bash
# The gate on the demo recording. Reads the visible commands out of
# scripts/docs/demo.tape, runs them in a fresh scene with no terminal
# involved, masks the fields that differ run to run — change ids, shas,
# ages — and diffs the result against scripts/docs/demo.golden.txt. A
# failure means the demo's commands or their output have moved and
# docs/assets/demo.gif no longer shows what fufu prints: re-render it with
# `make demo`, then re-bless this file.
#
#   scripts/docs/demo-check.sh            check, exit 1 on drift
#   scripts/docs/demo-check.sh --bless    rewrite the golden file
#
# The tape is the single source of the command list, so a command added to
# the recording is a command this checks, with no second list to keep in
# step. FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TAPE="$ROOT_DIR/scripts/docs/demo.tape"
GOLDEN="$ROOT_DIR/scripts/docs/demo.golden.txt"

FF="${FF:-ff}"

bless=false
case "${1:-}" in
  --bless) bless=true ;;
  '') ;;
  *) echo "usage: demo-check.sh [--bless]" >&2; exit 2 ;;
esac

# The tape's demo region, one command per `Type` line. A tape line reads
# `Type "cmd" Sleep 300ms Enter Sleep 3s`, and the command is the first
# quoted string on it; vhs takes ", ' or ` as the quote, so this does too.
commands() {
  local in_demo=false line quote rest
  while IFS= read -r line; do
    if [[ $line == '# --- demo ---' ]]; then in_demo=true; continue; fi
    $in_demo || continue
    [[ $line =~ ^Type\ (\"|\'|\`)(.*)$ ]] || continue
    quote=${BASH_REMATCH[1]}
    rest=${BASH_REMATCH[2]}
    printf '%s\n' "${rest%%"$quote"*}"
  done < "$TAPE"
}

# The tape types a bare `ff`, because that is what a reader types, so a
# binary named by FF is put on PATH under that name rather than substituted
# into the commands. CI runs this against the release build it just made.
if [ "$FF" != ff ]; then
  FF=$(cd "$(dirname "$FF")" && pwd)/$(basename "$FF")
  BIN_DIR=$(mktemp -d)
  ln -s "$FF" "$BIN_DIR/ff"
  PATH="$BIN_DIR:$PATH"
  export PATH
fi

# Hermetic, exactly as the tape's own shell is: the scene's commits carry a
# repository-local identity, and nothing else about this machine reaches the
# output.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false

SCENE_ROOT=$(FF="$FF" "$ROOT_DIR/scripts/docs/demo-scene.sh")
trap 'rm -rf "$(dirname "$SCENE_ROOT")" "${BIN_DIR:-}"' EXIT
cd "$SCENE_ROOT"

# The commands run through a shell so that the tape's quoting is the quoting
# under test, and stderr joins stdout because a verb's diagnostics are part
# of what the recording shows. A non-zero exit is the demo's own business —
# `git reset --hard` succeeding is not what this file is about — so only the
# text is compared.
transcript() {
  local cmd
  while IFS= read -r cmd; do
    # The blank line the tape's own prompt opens with, so that this file
    # reads the way the recording does.
    printf '\n$ %s\n' "$cmd"
    eval "$cmd" 2>&1 || true
  done < <(commands)
}

# The volatile fields, in the order that keeps each pattern unambiguous:
# shas first, so that a change id is then recognizable as the bare word
# standing in front of one.
mask() {
  sed -E \
    -e 's/\b[0-9a-f]{7,8}\b/<sha>/g' \
    -e 's/\b[0-9]+ ?[smhd] ago/<age> ago/g' \
    -e 's/\b[a-z]{8} <sha>/<id> <sha>/g' \
    -e 's/\bnow at [a-z]+\b/now at <id>/g'
}

out=$(transcript | mask)

if $bless; then
  printf '%s\n' "$out" > "$GOLDEN"
  echo "blessed $GOLDEN"
  exit 0
fi

if [ ! -f "$GOLDEN" ]; then
  echo "no golden file: run scripts/docs/demo-check.sh --bless" >&2
  exit 1
fi

if diff -u "$GOLDEN" <(printf '%s\n' "$out"); then
  echo "demo transcript matches $(basename "$GOLDEN")"
else
  cat >&2 <<'EOF'

the demo's output has drifted from the golden transcript. re-render the
recording with `make demo`, then re-bless with:

  scripts/docs/demo-check.sh --bless
EOF
  exit 1
fi
