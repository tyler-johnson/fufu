#!/usr/bin/env bash
# The source of truth for every console block in docs/guides/rewriting-history.md:
# builds a throwaway origin, then runs the guide's exact command sequence
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

# The same block for a command the guide expects to fail: the output is
# printed either way, and a success is the script's error, not the verb's.
show_refused() {
  local label='' word out
  for word in "$@"; do
    if [ "$word" = "$FF" ]; then
      word='ff'
    elif [[ "$word" == *' '* ]]; then
      word="\"$word\""
    fi
    label="$label${label:+ }$word"
  done
  printf '$ %s\n' "$label"
  if out=$("$@" 2>&1); then
    printf '%s\n' "$out"
    echo "expected a refusal from: $label" >&2
    exit 1
  fi
  printf '%s\n\n' "$out"
}

# --- the seed: a bare origin holding two starting commits ---
git init -q --bare -b main demo.git
git init -q -b main seed
(
  cd seed && ident
  mkdir src
  printf 'fn main() {\n    println!("hello world");\n}\n' > src/main.rs
  printf '# demo\n' > README.md
  git add -A && git commit -qm "init: hello world"
  printf '// v0.1.0\n' >> src/main.rs
  git add -A && git commit -qm "release: cut v0.1.0"
  git remote add origin ../demo.git && git push -q origin main
)
rm -rf seed

"$FF" clone "$SCENE/demo.git" > /dev/null 2>&1
cd demo && ident

# --- the scene: a named branch with two closed commits ---
"$FF" start > /dev/null 2>&1
"$FF" describe -b lexer > /dev/null 2>&1
printf 'fn lex() {}\nfn stream() {}\n' > src/lexer.rs
printf '// lexer wiring\n' >> src/main.rs
"$FF" commit -m "lexer: skeleton and stream" > /dev/null 2>&1
printf 'fn drop_whitespace() {}\n' >> src/lexer.rs
"$FF" commit -m "lexer: drop whitespace" > /dev/null 2>&1
show "$FF" log

# --- reword a closed commit ---
first=$(git rev-parse --short=8 HEAD~1)
show "$FF" describe "$first" -m "lexer: skeleton and char stream"

# --- fold the open change into a closed commit, path-filtered ---
# The helper goes at the top of the file, away from the tail the second
# commit appended to, so the restack above the absorb replays cleanly.
first=$(git rev-parse --short=8 HEAD~1)
{ printf 'fn helper() {}\n'; cat src/lexer.rs; } > src/lexer.rs.new
mv src/lexer.rs.new src/lexer.rs
printf '\nstray note\n' >> README.md
show "$FF" status
show "$FF" absorb src/lexer.rs --into "$first"
show "$FF" status
show "$FF" restore README.md

# --- reopen a closed commit: edit, then done ---
first=$(git rev-parse --short=8 HEAD~1)
show "$FF" edit "$first"
sed -i 's/fn helper() {}/fn helper(input: \&str) {}/' src/lexer.rs
show "$FF" status
show "$FF" done

# --- the same door, closed without landing ---
first=$(git rev-parse --short=8 HEAD~1)
show "$FF" edit "$first"
printf '// scratch\n' >> src/lexer.rs
show "$FF" done --abandon

# --- split at the close: paths close a slice ---
printf 'fn parse() {}\n' > src/parser.rs
printf '# notes\n' > NOTES.md
show "$FF" status
show "$FF" commit src/parser.rs -m "parser: entry point"
show "$FF" status
show "$FF" commit -m "notes: parser scratchpad"

# --- split a commit that already closed: lift, then close again ---
printf 'fn eat(c: char) {}\n' >> src/parser.rs
printf 'how eating chars works\n' >> NOTES.md
show "$FF" commit -m "parser: eat chars from the stream"
show "$FF"
show "$FF" lift NOTES.md
show "$FF"
show "$FF" status
show "$FF" commit -m "notes: eat chars notes"

# --- lifting everything drops the commit ---
show "$FF" lift
show "$FF" undo

# --- collide: would these two branches hit each other? ---
# The map orders branches newest tip first; the pause keeps renamer's tip
# strictly newer than lexer's so the column order holds run to run.
sleep 1
"$FF" start main > /dev/null 2>&1
"$FF" describe -b renamer > /dev/null 2>&1
printf 'fn rename() {}\n' > src/rename.rs
printf '// rename pass wiring\n' >> src/main.rs
"$FF" commit -m "renamer: rename pass" > /dev/null 2>&1
show "$FF"
show "$FF" collide lexer
tip=$(git rev-parse --short=8 HEAD)
show "$FF" lift src/main.rs --from "$tip"
show "$FF" collide lexer
show "$FF" restore src/main.rs
show "$FF" collide lexer
show "$FF"

# --- trim: retention of the operation log ---
show "$FF" trim -n
show "$FF" config keep 2s
# Age every operation past the two-second window, then touch each branch so
# its newest operations survive and the per-branch pointers stay put.
sleep 3
"$FF" switch lexer > /dev/null 2>&1
"$FF" switch main > /dev/null 2>&1
"$FF" switch renamer > /dev/null 2>&1
show "$FF" trim -n
show "$FF" trim
show "$FF"
show "$FF" history
"$FF" config keep 90d > /dev/null 2>&1

# --- the boundary: rewrites against a published branch ---
"$FF" switch lexer > /dev/null 2>&1
show "$FF" publish
tip=$(git rev-parse --short=8 HEAD)
show "$FF" describe "$tip" -m "notes: how eating chars works"
show "$FF" publish

# --- meanwhile: a teammate lands a commit on the shared copy ---
(
  git clone -q "$SCENE/demo.git" "$SCENE/mate"
  cd "$SCENE/mate" && ident
  git switch -q lexer
  printf '\nA note from a teammate.\n' >> README.md
  git add -A && git commit -qm "docs: teammate note"
  git push -q origin lexer
)

tip=$(git rev-parse --short=8 HEAD)
show "$FF" describe "$tip" -m "notes: eating chars, explained"
show_refused "$FF" publish
show "$FF" sync
