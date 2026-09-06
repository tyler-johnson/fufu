#!/usr/bin/env bash
# The demo's command list, in one place. Two things consume it and they must
# never drift apart: scripts/docs/casts.sh, which types it on camera into
# docs/assets/demo.cast, and scripts/docs/demo-check.sh, which replays it
# with no terminal at all against scripts/docs/demo.golden.txt.
#
# This file is sourced, not run. The lines use the tags of
# scripts/docs/tutorial-steps.sh: `run|` is a command, `note|` is narration,
# and the narration is a shell comment so that every line is a command that
# stands on its own when the check runs the list through bash. Short, and
# only where a line does not explain itself.
#
# The scene the lines run in is scripts/docs/demo-scene.sh's: work parked on
# a branch, different work open on main, and a teammate's commit waiting on
# origin.

demo_lines() {
  printf '%s\n' \
    "run|ff st" \
    "note|# a dirty working copy, but I need to switch branches" \
    "run|ff" \
    "run|ff switch unicode-escapes" \
    "run|ff st" \
    "note|# the changes I made earlier on this branch..." \
    "note|# looks like it's ready to commit" \
    "run|ff commit" \
    "note|# oops, forgot to add one thing" \
    "run|echo 'mod unicode;' >> src/lib.rs" \
    "run|ff absorb" \
    "run|ff st" \
    "note|# oh, main moved while I worked, so let's catch up" \
    "run|ff sync" \
    "note|# time to push my changes for review" \
    "run|ff publish" \
    "note|# what was I doing on main again?" \
    "run|ff switch main"
}
