#!/usr/bin/env bash
# JSON examples for docs/agents/machine-surface.md, in a scratch repository.
# FF names the binary under test. IDs, times, and paths vary between runs.
set -euo pipefail

FF="${FF:-ff}"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false CI=1
unset FF_SESSION CLAUDE_CODE_SESSION_ID
SCENE=$(mktemp -d)
trap 'rm -rf "$SCENE"' EXIT
cd "$SCENE"
git init -q -b main .
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config fufu.autoTrim false
git config fufu.updateCheck false
printf 'fn main() {}\n' > main.rs
"$FF" commit -m "parser: skeleton" > /dev/null
printf 'fn main() { parse(); }\n' > main.rs

show() {
  printf '$ ff %s --json | jq .\n' "$*"
  "$FF" "$@" --json | jq .
  echo
}

show status
show log -n 1
show evolog -n 1
show evolog HEAD
printf '// first pass\n' >> main.rs
"$FF" --session flight-3 trigger > /dev/null
printf '// second pass\n' >> main.rs
"$FF" --session flight-3 trigger > /dev/null
printf '$ ff history --json | jq -c '\''.data.steps[]'\''\n'
"$FF" history --json | jq -c '.data.steps[]'
printf '\n$ ff op log --json | jq -c '\''.data.ops[]'\''\n'
"$FF" op log --json | jq -c '.data.ops[]'
op=$("$FF" op log --json | jq -r '.data.ops[] | select(.verb == "commit") | .id')
show op show "$op"
printf '$ ff op log '\''session(flight-3)'\'' --json | jq -c '\''.data.ops[]'\''\n'
"$FF" op log 'session(flight-3)' --json | jq -c '.data.ops[]'
