#!/usr/bin/env bash
# The source of truth for every console block in docs/guides/stacked-changes.md:
# builds a throwaway origin with the tutorial's seed history, builds a
# two-branch stack, lands review feedback with absorb, cascades the branch
# above with restack after the absorb and again after a sync, and publishes
# each branch under its own lease. When a verb's output changes, run this and
# paste the new blocks rather than hand-editing them — ids and ages differ run
# to run, everything else must match.
#
# The file layout is deliberate: each commit in the stack owns its own region
# (a new file, or a distinct end of src/main.rs), so every cascade's replay of
# an already-rewritten copy merges clean and drops as empty. Overlap the hunks
# and the cascade holds on a conflict instead — which is `ff resolve`'s page,
# not this one.
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

"$FF" clone "$SCENE/demo.git" > /dev/null 2>&1
cd demo && ident

# --- the bottom branch: parser-core, forked from trunk ---
show "$FF" start -b parser-core
printf 'fn lex() {}\nfn stream() {}\nfn skeleton() {}\n' > src/parser.rs
show "$FF" commit -m "parser: skeleton and char stream"
{ printf 'mod parser;\n'; cat src/main.rs; } > src/main.rs.new
mv src/main.rs.new src/main.rs
show "$FF" commit -m "parser: wire the module into main"
printf 'fn buffered() {}\n' > src/stream.rs
show "$FF" commit -m "parser: buffered char stream"

# --- the branch above: parser-cli, forked at parser-core's tip ---
show "$FF" start parser-core -b parser-cli
printf 'fn parse_args() {}\nfn run() {}\n' > src/cli.rs
show "$FF" commit -m "cli: expose the parser behind a flag"

# --- the stack in the map ---
show "$FF"

# --- review feedback on the bottom branch ---
show "$FF" switch parser-core
# The note appends to src/main.rs, away from the top where the wiring commit
# put its line, so the absorb and every replay above it stay disjoint.
wiring=$(git rev-parse --short=8 HEAD~1)
printf '// the parser runs behind --parse\n' >> src/main.rs
show "$FF" absorb --into "$wiring"

# --- cascade: the branch above is reached by name, one branch at a time ---
show "$FF"
show "$FF" restack parser-cli
show "$FF"

# --- meanwhile: a teammate lands a commit on main ---
(
  git clone -q "$SCENE/demo.git" "$SCENE/teammate"
  cd "$SCENE/teammate" && ident
  printf 'A demo of fufu.\n' >> README.md
  git add -A && git commit -qm "docs: say what this is"
  git push -q origin main
)

# --- sync the branch you stand on; then cascade again ---
show "$FF" sync
show "$FF" restack parser-cli

# --- publish each branch under its own lease ---
show "$FF" publish
show "$FF" switch parser-cli
show "$FF" publish

# --- the finished stack ---
show "$FF"
