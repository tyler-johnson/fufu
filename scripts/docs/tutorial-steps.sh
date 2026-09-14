#!/usr/bin/env bash
# The tutorial's example scenes, shared by tutorial-transcript.sh and
# casts.sh. The page walks a reader through fufu's real repository and lets
# them make their own edits; these small hermetic fixtures exercise the
# same workflow without network access or writes to the project's remote.
# Fixture setup and literal editor substitutions are contributor tooling,
# not commands the tutorial reader needs to reproduce.
#
# This file is sourced, not run.
#
# A step is a function printing tagged lines, one per line:
#
#   run|<cmd>    a command the reader types: shown in the transcript, typed
#                on camera
#   edit|<cmd>   a file edit or fixture setup: shown in the transcript and
#                typed on camera as a stand-in for editing a file
#   note|# text  narration: typed on camera only
#   video|<cmd>  a read-only command a recording opens with so that it stands
#                on its own — where the page has prose and the section above
#                it, a video has only itself. Typed on camera, never run in
#                the transcript
#   set|<cmd>    scene machinery: silent in both,
#                and only ever at the head of a step, because a recording
#                clears the screen once and then never again
#   cont|<line>  a further line of the command above, for a heredoc
#
# A step runs with the working directory the step before it left, so a
# function is free to read the repository to build its own commands: the
# branch `ff start` created is looked up here rather than pasted anywhere.
#
# `ff` is spelled `ff` in every command; the callers put the binary under
# test on PATH under that name.

# The whole ordered sequence. VIDEO_STEPS is the subset that gets a
# recording: cloning a repository and glancing at it are steps a reader
# reads, not steps worth watching.
TUTORIAL_STEPS=(
  get-a-repository
  look-around
  start-work
  name-it-then-close-it
  switch-without-stashing
  fix-an-earlier-commit
  line-up-then-send
  undo-anything
)
TUTORIAL_VIDEO_STEPS=(
  start-work
  name-it-then-close-it
  switch-without-stashing
  fix-an-earlier-commit
  line-up-then-send
  undo-anything
)

# The commands all say `ff`, because that is what a reader types, so a
# binary named by FF goes on PATH under that name rather than being
# substituted into them. The caller removes TUTORIAL_BIN_DIR.
tutorial_put_ff_on_path() {
  local ff=$1
  [ "$ff" != ff ] || return 0
  ff=$(cd "$(dirname "$ff")" && pwd)/$(basename "$ff")
  TUTORIAL_BIN_DIR=$(mktemp -d)
  ln -s "$ff" "$TUTORIAL_BIN_DIR/ff"
  PATH="$TUTORIAL_BIN_DIR:$PATH"
  export PATH
}

step_get_a_repository() {
  printf '%s\n' \
    "edit|command git init -q -b main seed" \
    "edit|command git -C seed config user.name 'Tutorial Reader'" \
    "edit|command git -C seed config user.email reader@example.com" \
    "edit|printf '# Tutorial project\\n' > seed/README.md" \
    "edit|command git -C seed add README.md" \
    "edit|command git -C seed commit -qm 'docs: start the tutorial'" \
    "edit|command git clone -q --bare seed origin.git" \
    "run|ff clone ./origin.git exercise" \
    "edit|cd exercise" \
    "edit|command git config user.name 'Tutorial Reader'" \
    "edit|command git config user.email reader@example.com"
}

step_look_around() {
  printf '%s\n' \
    "run|ff"
}

step_start_work() {
  printf '%s\n' \
    "note|# a new branch from main, with an automatic name" \
    "run|ff start" \
    "edit|mkdir notes" \
    "edit|cat > notes/parser.md <<'EOF'" \
    "cont|A char stream feeds the lexer." \
    "cont|The lexer emits spans." \
    "cont|Whitespace is the stream's problem, not the lexer's." \
    "cont|EOF" \
    "note|# no add, no staging: the tree is the change" \
    "run|ff status"
}

step_name_it_then_close_it() {
  printf '%s\n' \
    "video|ff status" \
    "run|ff describe -m \"notes: parser skeleton and char stream\"" \
    "run|ff commit" \
    "note|# the next change opens by itself" \
    "edit|printf 'The lexer never sees whitespace.\\n' >> notes/parser.md" \
    "run|ff commit -m \"notes: drop whitespace from the stream\"" \
    "run|ff log -n 5"
}

step_switch_without_stashing() {
  # The branch `ff start` minted, whatever it was named this run.
  local minted
  minted=$(git branch --format='%(refname:short)' | grep -vx main | head -n1)
  printf '%s\n' \
    "edit|printf '\\nstray note\\n' >> README.md" \
    "note|# leave the unfinished edit on this branch" \
    "run|ff switch main" \
    "run|ff" \
    "run|ff switch $minted" \
    "note|# name the branch now that the work has a shape" \
    "run|ff describe -b parser-stream" \
    "note|# restore captures this edit before discarding it" \
    "run|ff restore README.md"
}

step_fix_an_earlier_commit() {
  # The commit the fix belongs to: the first of the two just made. The
  # heading goes at the top of the file, away from the tail the second
  # commit appended to, so the restack above the absorb replays cleanly —
  # the tutorial's absorb is the no-conflict one, and `ff resolve` has its
  # own page.
  printf '%s\n' \
    "video|ff log -n 3" \
    "note|# the heading belongs in the first commit, not a new one" \
    "edit|printf '# Parser notes\\n' | cat - notes/parser.md > notes/parser.md.new" \
    "edit|mv notes/parser.md.new notes/parser.md" \
    "run|ff absorb --into HEAD~1"
}

step_line_up_then_send() {
  printf '%s\n' \
    "set|command git clone -q ../origin.git ../teammate" \
    "set|command git -C ../teammate config user.name 'Tutorial Teammate'" \
    "set|command git -C ../teammate config user.email teammate@example.com" \
    "set|printf 'A line from a teammate.\\n' >> ../teammate/README.md" \
    "set|command git -C ../teammate commit -qam 'docs: a line from a teammate'" \
    "set|command git -C ../teammate push -q origin main" \
    "note|# a teammate landed on main while I worked" \
    "run|ff pull" \
    "note|# send the branch for review" \
    "run|ff push"
}

step_undo_anything() {
  printf '%s\n' \
    "note|# the preceding ff commands recorded the state to recover" \
    "run|git reset --hard HEAD~2" \
    "run|ff undo" \
    "run|ff history"
}

# The lines of one step, by id.
tutorial_step_lines() {
  "step_${1//-/_}"
}

# Runs a step. `transcript` prints commands, edits, and output for review;
# `quiet` runs the whole step for its
# effect, which is how a recording of a later step reaches its own starting
# state. `set` lines run silently in both.
tutorial_run_step() {
  local id=$1 mode=$2
  local -a lines
  mapfile -t lines < <(tutorial_step_lines "$id")

  local i=0 n=${#lines[@]} tag cmd
  while [ "$i" -lt "$n" ]; do
    tag=${lines[i]%%|*}
    cmd=${lines[i]#*|}
    i=$((i + 1))
    # A heredoc, or any other command spanning lines: the `cont` lines
    # under it are part of it.
    while [ "$i" -lt "$n" ] && [ "${lines[i]%%|*}" = cont ]; do
      cmd=$cmd$'\n'${lines[i]#*|}
      i=$((i + 1))
    done

    case "$tag" in
      # Camera only: narration, and the read-only command a recording opens
      # on so that it stands without the page around it.
      note|video) ;;
      # Scene machinery. `setup` is the scene builder asking for exactly
      # this and nothing else before recording starts.
      set) eval "$cmd" >/dev/null 2>&1 || return $? ;;
      edit|run)
        [ "$mode" != setup ] || continue
        case "$mode" in
          transcript)
            printf '$ %s\n' "$cmd"
            eval "$cmd" 2>&1 || return $?
            echo
            ;;
          # Run in this shell: edits can change its directory or variables.
          # Every mode fails on errors; check suppresses successful output.
          check)
            eval "$cmd" >/dev/null || {
              printf 'failed: $ %s\n' "$cmd" >&2
              return 1
            }
            ;;
          *) eval "$cmd" >/dev/null 2>&1 || return $? ;;
        esac
        ;;
    esac
  done
}
