#!/usr/bin/env bash
# The tutorial's command sequence, in one place. Two things consume it and
# they must never drift apart: scripts/docs/tutorial-transcript.sh, which
# prints the console blocks docs/tutorial.md carries, and
# scripts/docs/tutorial-tapes.sh, which records one video per section.
#
# This file is sourced, not run.
#
# A step is a function printing tagged lines, one per line:
#
#   run|<cmd>    a command the reader types: shown in the transcript, typed
#                on camera
#   edit|<cmd>   an edit standing in for opening an editor: silent in the
#                transcript, where the prose says what changed, and typed on
#                camera, where nothing can happen off screen
#   note|# text  narration: typed on camera only
#   video|<cmd>  a read-only command a recording opens with so that it stands
#                on its own — where the page has prose and the section above
#                it, a video has only itself. Typed on camera, never run in
#                the transcript
#   set|<cmd>    scene machinery — a teammate pushing, a cd: silent in both,
#                and only ever at the head of a step, because a recording
#                clears the screen once and then never again
#   cont|<line>  a further line of the command above, for a heredoc
#
# A step runs with the working directory the step before it left, so a
# function is free to read the repository to build its own commands: the
# branch `ff start` minted and the commit an absorb aims at are both looked
# up here rather than pasted anywhere.
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

# The origin every scene starts from: a bare, main-only copy of this
# repository, so `ff publish` pushes somewhere harmless.
tutorial_origin() {
  local scene=$1 root=$2
  git clone -q --bare --branch main --single-branch "$root" "$scene/fufu.git"
}

step_get_a_repository() {
  printf '%s\n' \
    "run|ff clone $SCENE/fufu.git" \
    "set|cd fufu" \
    "set|git config user.name 'Ada Lovelace'" \
    "set|git config user.email ada@example.com"
}

step_look_around() {
  printf '%s\n' \
    "run|ff"
}

step_start_work() {
  printf '%s\n' \
    "note|# a fresh branch off trunk, nothing to name yet" \
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
    "note|# mid-edit is fine: this parks, that resumes" \
    "run|ff switch main" \
    "run|ff" \
    "run|ff switch $minted" \
    "note|# name the branch now that the work has a shape" \
    "run|ff describe -b parser-stream" \
    "note|# and drop the stray edit" \
    "run|ff restore README.md"
}

step_fix_an_earlier_commit() {
  # The commit the fix belongs to: the first of the two just made. The
  # heading goes at the top of the file, away from the tail the second
  # commit appended to, so the restack above the absorb replays cleanly —
  # the tutorial's absorb is the no-conflict one, and `ff resolve` has its
  # own page.
  local first
  first=$(git rev-parse --short=8 HEAD~1)
  printf '%s\n' \
    "video|ff log -n 3" \
    "note|# the heading belongs in the first commit, not a new one" \
    "edit|printf '# Parser notes\\n' | cat - notes/parser.md > notes/parser.md.new" \
    "edit|mv notes/parser.md.new notes/parser.md" \
    "run|ff absorb --into $first"
}

step_line_up_then_send() {
  printf '%s\n' \
    "set|git clone -q $SCENE/fufu.git $SCENE/teammate" \
    "set|git -C $SCENE/teammate config user.name 'Grace Hopper'" \
    "set|git -C $SCENE/teammate config user.email grace@example.com" \
    "set|printf 'A line from a teammate.\\n' >> $SCENE/teammate/README.md" \
    "set|git -C $SCENE/teammate commit -qam 'docs: a line from a teammate'" \
    "set|git -C $SCENE/teammate push -q origin main" \
    "note|# a teammate landed on main while I worked" \
    "run|ff sync" \
    "note|# the one thing undo cannot take back" \
    "run|ff publish"
}

step_undo_anything() {
  printf '%s\n' \
    "note|# something done behind fufu's back, with raw git" \
    "run|git reset --hard HEAD~2" \
    "run|ff undo" \
    "run|ff history"
}

# The lines of one step, by id.
tutorial_step_lines() {
  "step_${1//-/_}"
}

# Runs a step. `transcript` prints each `run` line as a console block, the
# way docs/tutorial.md carries it; `quiet` runs the whole step for its
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
      # this and nothing else — the teammate's push has to have happened
      # before the recording of `ff sync` starts.
      set) eval "$cmd" >/dev/null 2>&1 || true ;;
      edit)
        [ "$mode" != setup ] || continue
        eval "$cmd" >/dev/null 2>&1 || true
        ;;
      run)
        [ "$mode" != setup ] || continue
        case "$mode" in
          transcript)
            printf '$ %s\n' "$cmd"
            eval "$cmd" 2>&1 || true
            echo
            ;;
          # The check mode is the one place a failing command matters: it is
          # how a renamed verb or a dropped flag is caught before a release
          # ships a video of it.
          check) eval "$cmd" >/dev/null 2>&1 || return 1 ;;
          *) eval "$cmd" >/dev/null 2>&1 || true ;;
        esac
        ;;
    esac
  done
}
