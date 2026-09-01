#!/usr/bin/env bash
# The source of truth for every console block in docs/tutorial.md: clones
# this repository itself into a throwaway scene — a bare, main-only copy
# serves as origin, so publish pushes somewhere harmless — then runs the
# tutorial's exact command sequence and prints the labeled transcript to
# stdout. When a verb's output changes, run this and paste the new blocks
# rather than hand-editing them; ids, ages, shas, and commit counts differ
# run to run, everything else must match. One hand-substitution is
# deliberate: the doc's clone line shows the public URL,
# https://github.com/tyler-johnson/fufu, where the transcript was captured
# against the scene-local bare copy. The transcripts follow the release:
# regenerate them when a release changes what the verbs print.
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

# --- the origin: a bare, main-only copy of this repository ---
git clone -q --bare --branch main --single-branch "$ROOT_DIR" "$SCENE/fufu.git"

# --- get a repository ---
show "$FF" clone "$SCENE/fufu.git"
cd fufu && ident

# The scene must never push back at the working checkout: confirm origin is
# the scene-local bare copy before anything below can reach for `publish`.
origin_url=$(git remote get-url origin)
case "$origin_url" in
  "$SCENE"/*) ;;
  *) echo "origin is not scene-local: $origin_url" >&2; exit 1 ;;
esac

# --- look around ---
show "$FF"

# --- start work ---
printf '$ %s\n' "ff start"
start_out=$("$FF" start 2>&1)
printf '%s\n\n' "$start_out"
minted=$(printf '%s\n' "$start_out" | sed -n 's/^minted \([^ ]*\).*/\1/p')
[ -n "$minted" ] || { echo "ff start minted no branch" >&2; exit 1; }

mkdir notes
printf '%s\n' "A char stream feeds the lexer." "The lexer emits spans." "Whitespace is the stream's problem, not the lexer's." > notes/parser.md
show "$FF" status

# --- name it, then close it ---
show "$FF" describe -m "notes: parser skeleton and char stream"
show "$FF" commit

printf 'The lexer never sees whitespace.\n' >> notes/parser.md
show "$FF" commit -m "notes: drop whitespace from the stream"
show "$FF" log -n 5

# --- switch without stashing ---
printf '\nstray note\n' >> README.md
show "$FF" switch main
show "$FF"
show "$FF" switch "$minted"
show "$FF" describe -b parser-stream
show "$FF" restore README.md

# --- fix an earlier commit ---
# The heading goes at the top of the file, away from the tail the second
# commit appended to, so the restack above the absorb replays cleanly — the
# tutorial's absorb is the no-conflict one, and `ff resolve` has its own page.
first=$(git rev-parse --short=8 HEAD~1)
{ printf '# Parser notes\n'; cat notes/parser.md; } > notes/parser.md.new
mv notes/parser.md.new notes/parser.md
show "$FF" absorb --into "$first"

# --- meanwhile: a teammate lands a commit on main ---
(
  git clone -q "$SCENE/fufu.git" "$SCENE/teammate"
  cd "$SCENE/teammate" && ident
  printf 'A line from a teammate.\n' >> README.md
  git add -A && git commit -qm "docs: a line from a teammate"
  git push -q origin main
)

# --- line up, then send ---
show "$FF" sync
show "$FF" publish

# --- undo anything ---
show git reset --hard HEAD~2
show "$FF" undo
show "$FF" history
