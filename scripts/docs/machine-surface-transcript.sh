#!/usr/bin/env bash
# JSON examples for docs/agents/machine-surface.md, in a scratch repository.
# FF names the binary under test. IDs, times, and paths vary between runs.
set -euo pipefail

FF="${FF:-ff}"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false CI=1
unset FF_SESSION CLAUDE_CODE_SESSION_ID GIT_AUTHOR_DATE GIT_COMMITTER_DATE
unset GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL EMAIL
SCENE=$(mktemp -d "${TMPDIR:-/tmp}/fufu-machine-XXXXXX")
trap 'rm -rf "$SCENE"' EXIT
cd "$SCENE"
git init -q -b main .
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config fufu.autoTrim false
git config fufu.updateCheck false
git config fufu.autoFetch false
printf 'fn main() {}\n' > main.rs
"$FF" commit -m "parser: skeleton" > /dev/null
printf 'fn main() { parse(); }\n' > main.rs

show() {
  local key=$1 filter=$2
  shift 2
  printf '<!-- transcript:%s -->\n```console\n' "$key"
  printf '$ ff %s --json | jq '\''%s'\''\n' "$*" "$filter"
  "$FF" "$@" --json | jq "$filter"
  printf '```\n<!-- /transcript -->\n\n'
}

# The same block cut by --fields rather than jq; jq only indents the line.
show_fields() {
  local key=$1 list=$2
  shift 2
  printf '<!-- transcript:%s -->\n```console\n' "$key"
  printf '$ ff %s --json --fields %s | jq .\n' "$*" "$list"
  "$FF" "$@" --json --fields "$list" | jq .
  printf '```\n<!-- /transcript -->\n\n'
}

show status '.data | {head, changes, held, resolving}' status
show_fields log 'commits,open.id,open.change_id,open.pending' log -n 1
show evolog '.data' evolog -n 1
show closed-evolog '.data | {change_id, commit, operations}' evolog HEAD
printf '// first pass\n' >> main.rs
"$FF" --session flight-3 trigger > /dev/null
printf '// second pass\n' >> main.rs
"$FF" --session flight-3 trigger > /dev/null
show history '.data | {floor, steps: .steps[:2]}' history
show op-log '.data.ops[:2]' op log
op=$("$FF" op log --json | jq -r '.data.ops[] | select(.verb == "commit") | .id')
show op-show '.data | {id, kind, summary, tree, refs}' op show "$op"
"$FF" op log 'session(flight-3)' --json | jq -e '.data.ops | length == 2 and all(.session == "flight-3" and .kind == "capture")' > /dev/null
printf '<!-- transcript:watch -->\n```console\n$ ff watch -n 1\n'
"$FF" watch -n 1
printf '```\n<!-- /transcript -->\n'
