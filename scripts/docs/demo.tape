# The source of the demo recording in the README and on the docs home page:
# a VHS tape (github.com/charmbracelet/vhs) that runs the real binary in a
# real terminal. `make demo` renders it to docs/assets/demo.gif and
# docs/assets/demo.webm; `make demo-check` replays the same commands with no
# terminal at all and fails when their output has drifted from the golden
# file, which is the signal to re-render.
#
# This tape is the demo's single source. demo-check.sh reads the `Type`
# lines out of it and runs them through bash, so every typed line must be a
# command that stands on its own — the narration lines are shell comments
# for exactly that reason.
#
# Rendering needs vhs, ttyd, and ffmpeg on PATH, and JetBrains Mono
# installed. Run it from the repository root.

Output docs/assets/demo.gif
Output docs/assets/demo.webm

Require ff
Require git

Set Shell bash
Set FontFamily "JetBrains Mono"
Set FontSize 20
Set Width 1200
Set Height 760
Set Padding 24
Set BorderRadius 10
Set WindowBar Colorful
Set Framerate 24
Set TypingSpeed 45ms

# GitHub's dark palette, so the frame sits flush against a README on
# github.com and reads as a terminal against the docs site's own light and
# dark themes. fufu's own colors are the ANSI eight, which these define.
Set Theme { "background": "#0d1117", "foreground": "#c9d1d9", "cursor": "#58a6ff", "selection": "#264f78", "black": "#484f58", "red": "#ff7b72", "green": "#3fb950", "yellow": "#d29922", "blue": "#58a6ff", "magenta": "#bc8cff", "cyan": "#39c5cf", "white": "#b1bac4", "brightBlack": "#6e7681", "brightRed": "#ffa198", "brightGreen": "#56d364", "brightYellow": "#e3b341", "brightBlue": "#79c0ff", "brightMagenta": "#d2a8ff", "brightCyan": "#56d4dd", "brightWhite": "#f0f6fc" }

# --- the scene: built off screen, and the tape never leaves it ---
# `Hide` stops the recording, not the terminal, so anything typed here would
# scroll into frame the moment the recording resumes. Only the cd, the
# prompt — which opens with a blank line, so that every command in the
# recording stands clear of the output above it — and the clear belong in
# here; the repository, the branch, and the
# edits the demo opens on are demo-scene.sh's job.
Hide
Type `export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_EDITOR=false EDITOR=false` Enter
Type `SCENE=$(scripts/docs/demo-scene.sh) && cd "$SCENE"` Enter
Type `PS1='\n\[\e[38;5;114m\]$\[\e[0m\] ' && unset PROMPT_COMMAND && clear` Enter
Sleep 1s
Show

# --- demo ---
# Everything below is read by demo-check.sh as well as by vhs: one typed
# line, one command, so replaying the list stays a plain bash run. The
# narration is shell comments for the same reason — short, and only where a
# line does not explain itself.

Sleep 1s
Type "ff st" Sleep 300ms Enter Sleep 3s

Type "# a dirty working copy, but I need to switch branches" Enter Sleep 700ms
Type "ff" Sleep 400ms Enter Sleep 1200ms
Type "ff switch unicode-escapes" Sleep 400ms Enter Sleep 2500ms

Type "ff st" Sleep 300ms Enter Sleep 3s

Type "# the changes I made earlier on this branch..." Enter Sleep 700ms
Type "# looks like it's ready to commit" Enter Sleep 600ms
Type "ff commit" Sleep 400ms Enter Sleep 2500ms

Type "# oops, forgot to add one thing" Enter Sleep 700ms
Type "echo 'mod unicode;' >> src/lib.rs" Sleep 400ms Enter Sleep 600ms
Type "ff absorb" Sleep 400ms Enter Sleep 2500ms

Type "ff st" Sleep 300ms Enter Sleep 3s

Type "# oh, main moved while I worked, so let's catch up" Enter Sleep 700ms
Type "ff sync" Sleep 400ms Enter Sleep 2500ms

Type "# time to push my changes for review" Enter Sleep 700ms
Type "ff publish" Sleep 400ms Enter Sleep 3s

Type "# what was I doing on main again?" Enter Sleep 700ms
Type "ff switch main" Sleep 400ms Enter Sleep 5s
