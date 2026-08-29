#!/usr/bin/env bash
# The source of truth for every console block in docs/guides/worktrees.md:
# builds a throwaway origin with the tutorial's seed history, then runs the
# guide's exact command sequence against it and prints the labeled transcript
# to stdout. When a verb's output changes, run this and paste the new blocks
# rather than hand-editing them — ids, ages, and temp paths differ run to
# run, everything else must match.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the transcript, and no
# editor ever opens (every commit carries its message).
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false

SCENE=$(mktemp -d)
trap 'rm -rf "$SCENE"' EXIT
cd "$SCENE"

ident() { git config user.name "Ada Lovelace"; git config user.email ada@example.com; }

# One console block: the command as the reader would type it, its output,
# a blank line. The label spells the binary `ff` whatever FF points at, and
# re-quotes arguments carrying spaces, so a block pastes into the docs as
# something a reader can type.
label() {
  local out='' word
  for word in "$@"; do
    if [ "$word" = "$FF" ]; then
      word='ff'
    elif [[ "$word" == *' '* ]]; then
      word="\"$word\""
    fi
    out="$out${out:+ }$word"
  done
  printf '%s' "$out"
}

show() {
  printf '$ %s\n' "$(label "$@")"
  "$@" 2>&1
  echo
}

# The same block for a command that must fail: the output is kept and the
# nonzero exit is demanded, not tolerated.
show_err() {
  printf '$ %s\n' "$(label "$@")"
  if "$@" 2>&1; then
    echo "expected this to fail: $(label "$@")" >&2
    exit 1
  fi
  echo
}

# --- the seed: a bare origin holding the tutorial's two starting commits ---
git init -q --bare -b main demo.git
git init -q -b main seed
(
  cd seed && ident
  printf 'fn main() {\n    println!("hello world");\n}\n' > src.rs
  mkdir src && mv src.rs src/main.rs
  printf '# demo\n' > README.md
  git add -A && git commit -qm "init: hello world"
  printf '// v0.1.0\n' >> src/main.rs
  git add -A && git commit -qm "release: cut v0.1.0"
  git remote add origin ../demo.git && git push -q origin main
)
rm -rf seed

# --- one checkout, then a second ---
show "$FF" clone "$SCENE/demo.git"
cd demo && ident
show "$FF" worktree
show "$FF" worktree add ../bay
show "$FF" worktree

# --- each tree has its own open change ---
cd "$SCENE/bay"
printf 'fn tokens() {}\n' > src/lexer.rs
show "$FF" status
show "$FF" commit -m "lexer: sketch the tokenizer"

cd "$SCENE/demo"
printf 'A demo of fufu.\n' >> README.md
show "$FF" commit -m "docs: say what this is"

# --- one repository, a log per tree ---
cd "$SCENE/bay"
printf 'fn spans() {}\n' >> src/lexer.rs
show "$FF" commit -m "lexer: emit spans"
show "$FF" undo
show "$FF" history

cd "$SCENE/demo"
show "$FF" history

# --- watching every tree ---
# The stream opens before the commit lands, so the block prints after the
# commit's own: the reader sees the verb, then what the already-open stream
# reported. -n 4 stops it after four events, counting the two opening ones.
"$FF" watch --all -n 4 > "$SCENE/watch.out" 2>&1 &
watch_pid=$!
for _ in $(seq 1 100); do
  starts=$(grep -c '"motion":"start"' "$SCENE/watch.out" 2>/dev/null || true)
  [ "${starts:-0}" -ge 2 ] && break
  sleep 0.1
done

cd "$SCENE/bay"
printf 'fn offsets() {}\n' >> src/lexer.rs
show "$FF" commit -m "lexer: spans and byte offsets"
wait "$watch_pid"
printf '$ %s\n' "ff watch --all -n 4"
cat "$SCENE/watch.out"
echo

# --- parking crosses trees with the branch ---
cd "$SCENE/demo"
printf '$ %s\n' "ff start"
start_out=$("$FF" start 2>&1)
printf '%s\n\n' "$start_out"
minted=$(printf '%s\n' "$start_out" | sed -n 's/^minted \([^ ]*\).*/\1/p')
[ -n "$minted" ] || { echo "ff start minted no branch" >&2; exit 1; }

printf 'a stray idea\n' > notes.md
show "$FF" switch main

cd "$SCENE/bay"
show "$FF" switch "$minted"

cd "$SCENE/demo"
show_err "$FF" switch "$minted"

cd "$SCENE/bay"
show "$FF" switch bay

# Not pasted into the guide: this block soaks up the parking churn the bay
# made, so demo's chain absorbs it here and the remove block below stays
# about removal.
cd "$SCENE/demo"
show "$FF" status

# --- remove captures first, and the chain outlives the checkout ---
cd "$SCENE/bay"
printf 'a half-written test\n' > src/lexer_test.rs

cd "$SCENE/demo"
printf '$ %s\n' "ff worktree remove bay"
remove_out=$("$FF" worktree remove bay 2>&1)
printf '%s\n\n' "$remove_out"
cap=$(printf '%s\n' "$remove_out" | sed -n 's/.*captured first as \([a-z]*\).*/\1/p')
[ -n "$cap" ] || { echo "remove reported no capture op" >&2; exit 1; }

show "$FF" worktree list
show "$FF" restore src/lexer_test.rs --at-op "$cap"
show "$FF" status
