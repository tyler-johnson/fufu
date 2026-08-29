#!/usr/bin/env bash
# The source of truth for every console block in docs/tutorial.md (and the
# guides that reuse its scenario): builds a throwaway origin with the
# tutorial's seed history, then runs the tutorial's exact command sequence
# against it and prints the labeled transcript to stdout. When a verb's
# output changes, run this and paste the new blocks rather than hand-editing
# them — ids and ages differ run to run, everything else must match.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the transcript, and no
# editor ever opens (every describe/commit carries its message).
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
show() {
  local label='' word
  for word in "$@"; do
    if [ "$word" = "$FF" ]; then
      word='ff'
    elif [[ "$word" == *' '* ]]; then
      word="\"$word\""
    fi
    label="$label${label:+ }$word"
  done
  printf '$ %s\n' "$label"
  "$@" 2>&1
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

# --- get a repository ---
show "$FF" clone "$SCENE/demo.git"
cd demo && ident

# --- look around ---
show "$FF"

# --- start work ---
printf '$ %s\n' "ff start"
start_out=$("$FF" start 2>&1)
printf '%s\n\n' "$start_out"
minted=$(printf '%s\n' "$start_out" | sed -n 's/^minted \([^ ]*\).*/\1/p')
[ -n "$minted" ] || { echo "ff start minted no branch" >&2; exit 1; }

printf 'fn lex() {}\nfn stream() {}\nfn skeleton() {}\n' > src/parser.rs
printf '// parser wiring\n' >> src/main.rs
show "$FF" status

# --- name it, then close it ---
show "$FF" describe -m "parser: skeleton and char stream"
show "$FF" commit

printf 'fn drop_whitespace() {}\n' >> src/parser.rs
show "$FF" commit -m "parser: drop whitespace from the stream"
show "$FF" log

# --- switch without stashing ---
printf '\nstray note\n' >> README.md
show "$FF" switch main
show "$FF"
show "$FF" switch "$minted"
show "$FF" describe -b parser-stream
show "$FF" restore README.md

# --- fix an earlier commit ---
# The helper goes at the top of the file, away from the tail the second
# commit appended to, so the restack above the absorb replays cleanly — the
# tutorial's absorb is the no-conflict one, and `ff resolve` has its own page.
first=$(git rev-parse --short=8 HEAD~1)
{ printf 'fn helper() {}\n'; cat src/parser.rs; } > src/parser.rs.new
mv src/parser.rs.new src/parser.rs
show "$FF" absorb --into "$first"

# --- meanwhile: a teammate lands a commit on main ---
(
  git clone -q "$SCENE/demo.git" "$SCENE/teammate"
  cd "$SCENE/teammate" && ident
  printf 'A demo of fufu.\n' >> README.md
  git add -A && git commit -qm "docs: say what this is"
  git push -q origin main
)

# --- line up, then send ---
show "$FF" sync
show "$FF" publish

# --- undo anything ---
show git reset --hard HEAD~2
show "$FF" undo
show "$FF" history
