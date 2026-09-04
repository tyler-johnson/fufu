#!/usr/bin/env bash
# Builds the throwaway repository the demo recording runs in and prints the
# path of the working checkout to stdout. The scene models a whole setup:
# an origin — a bare repository next door, which is all a remote has to be —
# a checkout of it with fufu turned on, a branch carrying a parked change,
# edits of your own sitting on main, and a commit a teammate landed on main
# while you were working, so `ff sync` has something real to take in.
#
# Both consumers of the demo start here: scripts/docs/demo.tape cds into the
# printed path before recording, and scripts/docs/demo-check.sh cds into it
# before replaying the tape's commands. Nothing here appears on screen, so
# it is free to be verbose; what the viewer sees begins at the tape's first
# visible command.
#
# The caller owns the scene and deletes it. FF names the binary under test;
# default is `ff` on PATH.
set -euo pipefail

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the scene, and no editor
# ever opens. The tape and the check export these into their own shell too,
# since the demo's own commands run there and not here.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false

SCENE=$(mktemp -d)
cd "$SCENE"

ident() { git config user.name "$1"; git config user.email "$2"; }

# --- the project the origin will hold ---
# Short paths and small files: every line of this has to read at a glance in
# a 1200-pixel-wide frame.
git init -q -b main seed
cd seed
ident "Ada Lovelace" ada@example.com

cat > README.md <<'EOF'
# lexer

A tokenizer for the toy language.
EOF
mkdir src
cat > src/lib.rs <<'EOF'
mod lexer;
mod token;
EOF
cat > src/lexer.rs <<'EOF'
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}
EOF
cat > src/token.rs <<'EOF'
pub enum Token {
    Ident,
    Number,
    Str,
}
EOF
git add -A
git commit -qm "seed the crate"

cat >> src/lexer.rs <<'EOF'

impl Lexer<'_> {
    pub fn next_token(&mut self) -> Token { todo!() }
}
EOF
git add -A
git commit -qm "lexer: a token at a time"

# --- the origin, and the checkout the demo is recorded in ---
cd "$SCENE"
git clone -q --bare seed origin.git
rm -rf seed
git clone -q origin.git lexer
cd lexer
ident "Ada Lovelace" ada@example.com
"$FF" init >/dev/null

# The state the demo opens in: work parked on a branch, and different work
# open on main. It happens here rather than on screen because `Hide` in a
# tape stops the recording, not the terminal — hidden commands still scroll
# into the frame the moment it resumes.
"$FF" start -b unicode-escapes >/dev/null
cat > src/unicode.rs <<'EOF'
pub fn unicode_escape(s: &str) -> char {
    todo!()
}
EOF
printf '    esc: bool,\n' >> src/lexer.rs
"$FF" switch main >/dev/null
printf '\nRun `cargo test` before you push.\n' >> README.md

# --- a teammate lands a commit on main, so sync has work to do ---
(
  cd "$SCENE"
  git clone -q origin.git teammate
  cd teammate
  ident "Grace Hopper" grace@example.com
  printf '\nBuilt for the toy language in `spec/`.\n' >> README.md
  git commit -qam "docs: say what this is for"
  git push -q origin main
)

# The demo's opening `ff status` says main is behind before anything has
# fetched, so the scene fetches: it is the state you are in the morning
# after, not something the recording has to spend a command on.
git fetch -q origin

echo "$SCENE/lexer"
