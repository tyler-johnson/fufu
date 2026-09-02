#!/usr/bin/env bash
# The source of truth for every console block in docs/guides/recovery.md:
# builds a throwaway origin, quietly reproduces the state each scenario
# starts from, then runs each scenario's exact command sequence and prints
# the labeled transcript to stdout. When a verb's output changes, run this
# and paste the new blocks rather than hand-editing them — ids, shas, and
# ages differ run to run, everything else must match. The `-- scenario --`
# marker lines separate scenarios for pasting; they are not console blocks.
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

# Same block, for a command that exits nonzero by design (a refusal is the
# output the scenario is about); anything else still aborts the run.
show_fails() {
  local label='' word rc=0
  for word in "$@"; do
    if [ "$word" = "$FF" ]; then
      word='ff'
    elif [[ "$word" == *' '* ]]; then
      word="\"$word\""
    fi
    label="$label${label:+ }$word"
  done
  printf '$ %s\n' "$label"
  "$@" 2>&1 || rc=$?
  [ "$rc" -ne 0 ] || { echo "expected a refusal, got exit 0: $label" >&2; exit 1; }
  echo
}

mark() { printf -- '-- %s --\n\n' "$1"; }

# --- the scene, built quietly: the tutorial's repository, a few commits in ---
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
"$FF" start > /dev/null
printf 'fn lex() {}\nfn stream() {}\nfn skeleton() {}\n' > src/parser.rs
printf '// parser wiring\n' >> src/main.rs
"$FF" commit -m "parser: skeleton and char stream" > /dev/null
"$FF" describe -b parser-stream > /dev/null
printf 'fn drop_whitespace() {}\n' >> src/parser.rs
"$FF" commit -m "parser: drop whitespace from the stream" > /dev/null

# --- scenario: an agent ran `git reset --hard` ---
mark "an agent ran git reset --hard"
show git reset --hard HEAD~2
show "$FF" undo

# --- scenario: one file back the way it was ---
mark "one file back"
printf 'fn lex() { totally rewritten, and wrong }\n' > src/parser.rs
show "$FF" restore src/parser.rs
show "$FF" restore src/main.rs --from main
"$FF" restore src/main.rs > /dev/null   # quietly put the branch's version back

# --- scenario: the whole tree from twenty minutes ago ---
mark "the whole tree from earlier"
# A refactor goes sideways across the tree; a status runs somewhere in
# between, so the wreckage is captured too.
printf 'fn lex(input: &Stream) -> ! { unimplemented!() }\n' > src/parser.rs
printf 'fn main() { compile_error!("mid-refactor") }\n' > src/main.rs
"$FF" status > /dev/null
show "$FF" history
good=$("$FF" op log | awk '/ op /&&/commit on parser-stream: parser: drop whitespace/{print $1; exit}')
[ -n "$good" ] || { echo "no commit operation found in ff op log" >&2; exit 1; }
show "$FF" op show "$good"
show "$FF" op restore "$good"

# --- scenario: I undid too far ---
mark "undid too far, redo"
show "$FF" undo
show "$FF" redo

mark "landing new work forks the redo path"
show "$FF" undo
printf 'fn drop_comments() {}\n' >> src/parser.rs   # the reopened whitespace edit is already in the tree
show "$FF" commit -m "parser: drop whitespace and comments"
show_fails "$FF" redo

# --- scenario: two writers on one chain, and only one was wrong ---
mark "two writers on one chain"
# Writer A and writer B share this worktree, so their operations land on
# one chain in turn: A commits on parser-stream, B starts a branch off
# trunk and commits there. A's commit is the wrong one; B's must stand.
printf '\nThe parser lives in src/parser.rs.\n' >> README.md
"$FF" commit -m "README: point at the parser" > /dev/null
"$FF" start -b changelog > /dev/null
printf '# changelog\n' > CHANGELOG.md
"$FF" commit -m "changelog: start one" > /dev/null
show "$FF" op log -n 6
wrong=$("$FF" op log | awk '/ op /&&/commit on parser-stream: README: point at the parser/{print $1; exit}')
[ -n "$wrong" ] || { echo "no README commit operation found in ff op log" >&2; exit 1; }
show "$FF" op revert "$wrong"
"$FF" switch parser-stream > /dev/null   # quietly back to the branch the rest of the page works on

# --- scenario: wrong message (wrong branch is prose + pointers) ---
mark "wrong message"
printf 'fn string_literal() {}\n' >> src/parser.rs
show "$FF" commit -m "wip"
sha=$(git rev-parse --short=8 HEAD)
show "$FF" describe "$sha" -m "parser: string literals"

# --- scenario: someone force-pushed over my branch ---
mark "force-pushed over my branch"
show "$FF" publish
(
  git clone -q "$SCENE/demo.git" -b parser-stream "$SCENE/teammate"
  cd "$SCENE/teammate" && ident
  git commit -q --amend -m "parser: string literals (cleaned up)"
  git push -qf origin parser-stream
)
printf 'fn escape_sequence() {}\n' >> src/parser.rs
"$FF" commit -m "parser: escape sequences" > /dev/null
show_fails "$FF" publish
show "$FF" sync
show "$FF" publish

# --- scenario: what undo cannot reach — the floor ---
mark "the floor: a repository fufu just adopted"
mkdir "$SCENE/legacy"
cd "$SCENE/legacy" && git init -q -b main . && ident
printf 'years of history\n' > notes.txt
git add -A && git commit -qm "old work, made before fufu arrived"
show "$FF" init
show "$FF" history
