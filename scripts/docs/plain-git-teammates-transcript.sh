#!/usr/bin/env bash
# The source of truth for every console block in
# docs/guides/plain-git-teammates.md: builds a throwaway origin, then runs
# the guide's exact command sequence against it — a parked change seen from
# plain git, a raw git write absorbed at the next fufu verb, `ff git` as the
# captured escape hatch, and strict mode refusing — and prints the labeled
# transcript to stdout. When a verb's output changes, run this and paste the
# new blocks rather than hand-editing them — ids and ages differ run to run,
# everything else must match.
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

# The same block for a command that is expected to be refused: the guide's
# strict-mode scene ends in exit 2, which must not end the script.
show_denied() {
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
  if "$@" 2>&1; then
    echo "expected a refusal, got success: $label" >&2
    exit 1
  fi
  echo
}

# --- the seed: a bare origin holding one starting commit ---
git init -q --bare -b main demo.git
git init -q -b main seed
(
  cd seed && ident
  printf 'fn main() {\n    println!("hello world");\n}\n' > main.rs
  printf '# demo\n' > README.md
  git add -A && git commit -qm "init: hello world"
  git remote add origin ../demo.git && git push -q origin main
)
rm -rf seed

"$FF" clone "$SCENE/demo.git" > /dev/null
cd demo && ident

# --- a branch with a commit on it, made through fufu ---
show "$FF" start -b parser
printf 'fn lex() {}\n' > lexer.rs
show "$FF" commit -m "lexer: skeleton"

# --- what a parked change looks like from plain git ---
printf '// tuning pass\n' >> main.rs
show "$FF" switch main
show git stash list
"$FF" switch parser > /dev/null 2>&1
"$FF" restore main.rs > /dev/null

# --- a raw git write, absorbed at the next fufu verb ---
printf 'A demo repo.\n' >> README.md
show git commit -am "docs: say what this is"
show "$FF" status

# --- ff git: captured first, run verbatim, undoable ---
show "$FF" git reset --hard HEAD~1
show "$FF" undo

# --- strict mode refuses the words fufu has verbs for ---
show "$FF" config gitPolicy strict
show_denied "$FF" git commit -m "wip"
show "$FF" git log --oneline -n 2
