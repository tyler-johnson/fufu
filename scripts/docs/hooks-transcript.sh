#!/usr/bin/env bash
# The source of truth for every console and file block in
# docs/reference/hooks/*.md: builds a throwaway home with an empty rc file
# per shell and an empty config directory per agent client, then runs
# `ff hook <slug>` and `ff unhook <slug>` for each of the eight slugs and
# prints what each one said and what the files held afterward. When a
# slug's output or wiring changes, run this and paste the new blocks rather
# than hand-editing them. The `-- slug --` marker lines separate slugs for
# pasting; they are not console blocks.
#
# The temp home prints as `~`, and the claude plugin's baked binary path
# (whatever binary ran `ff hook`) prints as /usr/local/bin/ff, so the blocks
# read the same on every machine.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the transcript, no editor
# ever opens, and no rc file, shell, or client outside the scene is read.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false
unset ZDOTDIR XDG_CONFIG_HOME
# `ff hook claude` peeks at a non-terminal stdin for a legacy hook payload;
# a closed stdin is the hermetic answer.
exec < /dev/null

SCENE=$(mktemp -d)
trap 'rm -rf "$SCENE"' EXIT

export HOME="$SCENE/home"
mkdir -p "$HOME/.config/fish" "$HOME/.config/powershell" "$HOME/.claude" "$HOME/.codex" "$HOME/.cursor" "$HOME/.gemini"
: > "$HOME/.bashrc"
: > "$HOME/.zshrc"
: > "$HOME/.config/fish/config.fish"
: > "$HOME/.config/powershell/Microsoft.PowerShell_profile.ps1"

# A repository to stand in, in case a verb wants one.
mkdir "$SCENE/repo"
cd "$SCENE/repo"
git init -q -b main .
git config user.name "Ada Lovelace"; git config user.email ada@example.com
"$FF" init > /dev/null 2>&1

# Every path under the scene's home prints as `~`; the plugin's baked
# binary path prints as a stable one.
FF_ABS=$(command -v "$FF" || printf '%s' "$FF")
FF_ABS=$(readlink -f "$FF_ABS" 2>/dev/null || printf '%s' "$FF_ABS")
tidy() { sed -e "s|$HOME|~|g" -e "s|$FF_ABS|/usr/local/bin/ff|g"; }

# One console block: the command as the reader would type it, its output,
# a blank line. The label spells the binary `ff` whatever FF points at.
show() {
  local label='' word
  for word in "$@"; do
    if [ "$word" = "$FF" ]; then
      word='ff'
    fi
    label="$label${label:+ }$word"
  done
  printf '$ %s\n' "$label" | tidy
  "$@" 2>&1 | tidy
  echo
}

# The files under a directory fufu owns, sorted so the block is stable.
list_files() {
  printf '$ find %s -type f | sort\n' "${1/#$HOME/\~}"
  find "$1" -type f | sort | tidy
  echo
}

# A file as a `$ cat` block, with the home rewritten.
cat_file() {
  local path=$1
  printf '$ cat %s\n' "${path/#$HOME/\~}"
  tidy < "$path"
  echo
}

mark() { printf -- '-- %s --\n\n' "$1"; }

# --- the shells: two or three marked lines in an rc file ---
# powershell's rc is the Linux path; the page gives the Windows ones in
# prose.
for shell in bash zsh fish powershell; do
  case $shell in
    bash) rc="$HOME/.bashrc" ;;
    zsh) rc="$HOME/.zshrc" ;;
    fish) rc="$HOME/.config/fish/config.fish" ;;
    powershell) rc="$HOME/.config/powershell/Microsoft.PowerShell_profile.ps1" ;;
  esac
  mark "$shell"
  show "$FF" hook "$shell"
  cat_file "$rc"
  show "$FF" unhook "$shell"
  cat_file "$rc"
done

# --- claude: a plugin directory, written whole ---
mark claude
show "$FF" hook claude
list_files "$HOME/.claude/skills/fufu"
cat_file "$HOME/.claude/skills/fufu/hooks/hooks.json"
show "$FF" unhook claude
list_files "$HOME/.claude"

# --- codex, cursor, gemini: entries merged into a settings file ---
mark codex
show "$FF" hook codex
cat_file "$HOME/.codex/hooks.json"
list_files "$HOME/.codex"
show "$FF" unhook codex
cat_file "$HOME/.codex/hooks.json"
list_files "$HOME/.codex"

mark cursor
show "$FF" hook cursor
cat_file "$HOME/.cursor/hooks.json"
show "$FF" unhook cursor
cat_file "$HOME/.cursor/hooks.json"

mark gemini
show "$FF" hook gemini
cat_file "$HOME/.gemini/settings.json"
show "$FF" unhook gemini
cat_file "$HOME/.gemini/settings.json"
