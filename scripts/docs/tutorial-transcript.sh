#!/usr/bin/env bash
# Replay the tutorial workflow in a tiny repository with a disposable local
# origin. The output is evidence for reviewing the illustrative page, not
# a line-for-line specification for it: readers clone fufu's real repository,
# choose their own edits, and adapt IDs and output. Keep fixture setup here
# rather than teaching it as a prerequisite. Rerun when a release changes
# the verbs and reconcile the examples for behavior and clarity.
#
# The sequence itself lives in scripts/docs/tutorial-steps.sh, which
# scripts/docs/casts.sh records one clip per section from. Editing the
# tutorial's commands means editing that file, and both follow.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the transcript, and no
# editor ever opens (every describe/commit carries its message).
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=fufu.updateCheck GIT_CONFIG_VALUE_0=false
# Nor the session of whatever launched this: an operation records the one
# it ran under, and a recording made from inside an agent's session would
# carry that session's id on every `ff history` row.
unset FF_SESSION CLAUDE_CODE_SESSION_ID

# shellcheck source=scripts/docs/tutorial-steps.sh
. "$ROOT_DIR/scripts/docs/tutorial-steps.sh"

tutorial_put_ff_on_path "$FF"

SCENE=$(mktemp -d)
trap 'rm -rf "$SCENE" "${TUTORIAL_BIN_DIR:-}"' EXIT
cd "$SCENE"

for step in "${TUTORIAL_STEPS[@]}"; do
  tutorial_run_step "$step" transcript

  # The scene must never push back at the working checkout: confirm origin
  # is the scene-local bare copy before anything below can reach for
  # `push`.
  if [ "$step" = get-a-repository ]; then
    origin_url=$(git remote get-url origin)
    case "$origin_url" in
      "$SCENE"/*) ;;
      *) echo "origin is not scene-local: $origin_url" >&2; exit 1 ;;
    esac
  fi
done
