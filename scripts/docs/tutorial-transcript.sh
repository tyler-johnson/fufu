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
# The sequence itself lives in scripts/docs/tutorial-steps.sh, which
# scripts/docs/tutorial-tapes.sh records one video per section from. Editing
# the tutorial's commands means editing that file, and both follow.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

FF="${FF:-ff}"

# Hermetic: no user or system git config reaches the transcript, and no
# editor ever opens (every describe/commit carries its message).
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false

# shellcheck source=scripts/docs/tutorial-steps.sh
. "$ROOT_DIR/scripts/docs/tutorial-steps.sh"

tutorial_put_ff_on_path "$FF"

SCENE=$(mktemp -d)
trap 'rm -rf "$SCENE" "${TUTORIAL_BIN_DIR:-}"' EXIT
cd "$SCENE"

tutorial_origin "$SCENE" "$ROOT_DIR"

for step in "${TUTORIAL_STEPS[@]}"; do
  tutorial_run_step "$step" transcript

  # The scene must never push back at the working checkout: confirm origin
  # is the scene-local bare copy before anything below can reach for
  # `publish`.
  if [ "$step" = get-a-repository ]; then
    origin_url=$(git remote get-url origin)
    case "$origin_url" in
      "$SCENE"/*) ;;
      *) echo "origin is not scene-local: $origin_url" >&2; exit 1 ;;
    esac
  fi
done
