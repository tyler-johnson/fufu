#!/usr/bin/env bash
# Records the docs' terminal recordings: the demo on the README and the
# docs home page, and one clip per tutorial section. Each is an asciicast
# under docs/assets/, written by scripts/docs/cast.py typing the step's
# lines into a real shell, and a gif beside it rendered by agg.
#
#   scripts/docs/casts.sh                    everything
#   scripts/docs/casts.sh demo               the demo alone
#   scripts/docs/casts.sh start-work         one tutorial clip
#   scripts/docs/casts.sh --check            no recording: replay every
#                                            tutorial step and fail on a
#                                            command that no longer works
#
# The demo's lines come from scripts/docs/demo-steps.sh, which
# scripts/docs/demo-check.sh replays against its golden transcript, and the
# tutorial's from scripts/docs/tutorial-steps.sh, which the page's
# transcripts come from, so a recording cannot drift from what the checks
# and the page carry. Only --check runs in CI; rendering wants agg on PATH
# and JetBrains Mono installed, and is something a release does by hand.
#
# FF names the binary under test; default is `ff` on PATH.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
ASSETS="$ROOT_DIR/docs/assets"

FF="${FF:-ff}"

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_EDITOR=false EDITOR=false

# shellcheck source=scripts/docs/tutorial-steps.sh
. "$ROOT_DIR/scripts/docs/tutorial-steps.sh"
# shellcheck source=scripts/docs/demo-steps.sh
. "$ROOT_DIR/scripts/docs/demo-steps.sh"

check=false
if [ "${1:-}" = --check ]; then
  check=true
  shift
fi

targets=("$@")
[ ${#targets[@]} -gt 0 ] || targets=(demo "${TUTORIAL_VIDEO_STEPS[@]}")

tutorial_put_ff_on_path "$FF"

# --- the check: every tutorial step, in order, with nothing recorded ---
if $check; then
  scene=$(mktemp -d)
  trap 'rm -rf "$scene" "${TUTORIAL_BIN_DIR:-}"' EXIT
  tutorial_origin "$scene" "$ROOT_DIR"
  (
    SCENE=$scene
    cd "$scene"
    for step in "${TUTORIAL_STEPS[@]}"; do
      tutorial_run_step "$step" check || {
        echo "tutorial step '$step' no longer runs clean" >&2
        exit 1
      }
    done
  )
  echo "every tutorial step runs"
  exit 0
fi

command -v agg >/dev/null || { echo "agg is not on PATH: cargo install --git https://github.com/asciinema/agg" >&2; exit 1; }

# The recorder and the gif renderer, each in one place so every recording
# in the docs looks like one machine. The theme is in the cast's header,
# where agg and the site's player both read it.
record() {
  local cwd=$1 cast=$2
  python3 "$ROOT_DIR/scripts/docs/cast.py" --cwd "$cwd" --out "$cast"
}
gif() {
  local cast=$1 gif=$2
  agg --font-family "JetBrains Mono" --font-size 20 --fps-cap 24 \
    --last-frame-duration 3 --quiet "$cast" "$gif"
}

# One scene per tutorial clip. A step's clip opens on the state its section
# opens on, which is every earlier step run for its effect and nothing of
# them on screen.
scene_up_to() {
  local target=$1 scene step
  scene=$(mktemp -d)
  tutorial_origin "$scene" "$ROOT_DIR"
  (
    SCENE=$scene
    cd "$scene"
    for step in "${TUTORIAL_STEPS[@]}"; do
      if [ "$step" = "$target" ]; then
        # The step's own machinery — a teammate landing a commit — belongs
        # to the state the recording opens on, not to the recording.
        tutorial_run_step "$step" setup
        break
      fi
      tutorial_run_step "$step" quiet
    done
    # Where the earlier steps left off, which is the directory the
    # recording opens in.
    pwd > "$scene/.cwd"
  )
  printf '%s\n' "$scene"
}

record_demo() {
  local scene
  scene=$(FF="$FF" "$ROOT_DIR/scripts/docs/demo-scene.sh")
  echo "recording demo"
  demo_lines | record "$scene" "$ASSETS/demo.cast"
  gif "$ASSETS/demo.cast" "$ASSETS/demo.gif"
  rm -rf "$(dirname "$scene")"
}

record_step() {
  local step=$1 scene cwd
  scene=$(scene_up_to "$step")
  cwd=$(cat "$scene/.cwd")
  echo "recording $step"
  # The step's own lines, asked for from inside the scene and with SCENE
  # set: a step looks up the branch `ff start` minted and the commit an
  # absorb aims at in the repository in front of it, and that repository
  # is the scene, never this one.
  (cd "$cwd" && SCENE=$scene tutorial_step_lines "$step") |
    record "$cwd" "$ASSETS/tutorial/$step.cast"
  gif "$ASSETS/tutorial/$step.cast" "$ASSETS/tutorial/$step.gif"
  rm -rf "$scene"
}

mkdir -p "$ASSETS/tutorial"
trap 'rm -rf "${TUTORIAL_BIN_DIR:-}"' EXIT

for target in "${targets[@]}"; do
  if [ "$target" = demo ]; then
    record_demo
  else
    record_step "$target"
  fi
done

echo "wrote ${#targets[@]} recording(s) under $ASSETS"
