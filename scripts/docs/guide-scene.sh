#!/usr/bin/env bash
# Shared scratch fixtures for task-guide transcripts, independent of recordings.
# Source this from a guide script. An optional argument selects one recipe.
set -euo pipefail
FF=$(command -v "${FF:-ff}")
export FF
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_COUNT=3 GIT_CONFIG_KEY_0=fufu.updateCheck GIT_CONFIG_VALUE_0=false
export GIT_CONFIG_KEY_1=fufu.autoFetch GIT_CONFIG_VALUE_1=false
export GIT_CONFIG_KEY_2=fufu.autoTrim GIT_CONFIG_VALUE_2=false
export GIT_EDITOR=false EDITOR=false LC_ALL=C TZ=UTC
unset FF_SESSION CLAUDE_CODE_SESSION_ID OPENCODE_SESSION_ID
SCENE=$(mktemp -d "${TMPDIR:-/tmp}/fufu-guide-XXXXXX")
trap 'rm -rf "$SCENE"' EXIT
SELECT=${1:-}

ff() { "$FF" "$@"; }
ident() {
    git config user.name 'Ada Lovelace' || exit
    git config user.email ada@example.com || exit
}

fresh() {
    local name=$1
    [[ -z "$SELECT" || "$SELECT" == "$name" ]] || return 1
    CASE="$SCENE/$name"
    # This function is used as an if condition, where Bash disables errexit.
    # Every setup failure must explicitly abort before any task can run.
    mkdir -p "$CASE/demo" || exit
    cd "$CASE/demo" || exit
    git init -q -b main || exit
    ident
    printf 'hello\n' > app.txt || exit
    printf '# demo\n' > README.md || exit
    git add . || exit
    git commit -qm 'demo: initial files' || exit
    ff init >/dev/null || exit
    ff switch -b feature >/dev/null || exit
    printf '<!-- transcript:%s -->\n```console\n' "$name"
}

run() {
    local command=$1 expected=${2:-0} rc=0
    printf '$ %s\n' "$command"
    eval "$command" >"$SCENE/command.out" 2>&1 || rc=$?
    LAST=$(<"$SCENE/command.out")
    [[ -z "$LAST" ]] || printf '%s\n' "$LAST"
    printf '\n'
    if [[ "$rc" != "$expected" ]]; then
        printf 'Expected exit %s, got %s: %s\n' "$expected" "$rc" "$command" >&2
        exit 1
    fi
}

end() { printf '```\n<!-- /transcript -->\n'; }
tip() { git rev-parse "$@"; }
op() { git rev-parse refs/fufu/wt/main/ops; }
content() { [[ "$(git show "$1")" == "$2" ]]; }
absent_object() {
    if git cat-file -e "$1" 2>/dev/null; then
        printf 'Expected no object at %s\n' "$1" >&2
        exit 1
    fi
}
absent_ref() {
    if git show-ref --verify --quiet "$1"; then
        printf 'Expected no ref at %s\n' "$1" >&2
        exit 1
    fi
}

# A same-machine remote and plain-Git teammate; only scratch refs are sent.
remote() {
    git init -q --bare -b main "$CASE/origin.git"
    git remote add origin "$CASE/origin.git"
    git push -qu origin main
    git clone -q "$CASE/origin.git" "$CASE/teammate"
    git -C "$CASE/teammate" config user.name 'Grace Hopper'
    git -C "$CASE/teammate" config user.email grace@example.com
}
