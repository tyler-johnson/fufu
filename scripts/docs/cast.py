#!/usr/bin/env python3
"""Record a scripted terminal session as an asciicast.

The camera for every recording in the docs. It starts bash on a pty it
owns, types the on-camera lines of a step one character at a time, and
writes what the terminal printed as an asciicast v2 file, which agg renders
to a gif and the asciinema player replays on the site. No terminal emulator
and no browser is involved: the cast is the bytes the shell wrote, with a
timestamp on each read.

    cast.py --cwd DIR --out FILE [--cols N] [--rows N] < tagged-lines

The lines on stdin use scripts/docs/tutorial-steps.sh's grammar. `run`,
`video`, `edit`, `note` and `cont` lines are typed on camera; `set` lines
are scene machinery the scene scripts have already run, and never reach the
terminal. Each tag carries its own pacing: a pause before Enter, then a
dwell once the prompt is back. The dwell counts from the prompt rather than
from Enter, so a slow verb and a fast one both leave the same beat of
silence for the viewer to read.

Before the clock starts, a preamble makes the shell hermetic, cds into the
scene, sets the prompt and clears the screen. The first event of the cast
is that prompt, so the recording opens on an empty terminal with `$ ` on
its second line.
"""

import argparse
import codecs
import fcntl
import json
import os
import pty
import select
import struct
import sys
import termios
import time

# GitHub's dark palette, so the frame sits flush against a README on
# github.com and reads as a terminal against the docs site's own light and
# dark themes. fufu's own colors are the ANSI eight, which these define.
# Both agg and the player read the theme out of the cast's header, so it is
# defined here and nowhere else.
THEME = {
    "fg": "#c9d1d9",
    "bg": "#0d1117",
    "palette": ":".join(
        [
            "#484f58", "#ff7b72", "#3fb950", "#d29922",
            "#58a6ff", "#bc8cff", "#39c5cf", "#b1bac4",
            "#6e7681", "#ffa198", "#56d364", "#e3b341",
            "#79c0ff", "#d2a8ff", "#56d4dd", "#f0f6fc",
        ]
    ),
}

# The prompt, and the bytes that identify it in the output. It opens with a
# blank line so that every command stands clear of the output above it.
PS1 = r"\n\[\e[38;5;114m\]$\[\e[0m\] "
PROMPT = b"\x1b[38;5;114m$\x1b[0m "

TYPING_DELAY = 0.045
# Seconds before Enter and seconds after the prompt returns, per tag.
PAUSE = {"run": 0.4, "video": 0.4, "edit": 0.2, "cont": 0.1, "note": 0.0}
DWELL = {"run": 2.5, "video": 2.5, "edit": 0.5, "cont": 0.3, "note": 0.9}
# Silence at the start, before the first key, and at the end, after the last
# dwell.
LEAD_IN = 1.0
TAIL = 2.0
PROMPT_TIMEOUT = 30.0


class Recorder:
    def __init__(self, cwd, cols, rows, out):
        self.cols, self.rows = cols, rows
        self.out = out
        self.events = []
        self.t0 = None
        self.decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
        # Output since the last Enter that expects a prompt, for detection.
        self.since_enter = bytearray()
        self.alive = True

        pid, fd = pty.fork()
        if pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
            env = dict(os.environ, TERM="xterm-256color")
            os.execvpe("bash", ["bash", "--norc", "--noprofile"], env)
        self.pid, self.fd = pid, fd
        self.cwd = cwd

    # --- the pty ---

    def _read(self):
        try:
            data = os.read(self.fd, 65536)
        except OSError:
            data = b""
        if not data:
            self.alive = False
            return
        self.since_enter += data
        if self.t0 is not None:
            self._emit(data)

    def _emit(self, data):
        text = self.decoder.decode(data)
        if text:
            self.events.append((time.monotonic() - self.t0, text))

    def pump(self, seconds):
        """Read output for `seconds`, recording whatever arrives."""
        deadline = time.monotonic() + seconds
        while self.alive:
            left = deadline - time.monotonic()
            if left <= 0:
                break
            ready, _, _ = select.select([self.fd], [], [], left)
            if ready:
                self._read()

    def wait_prompt(self, what):
        deadline = time.monotonic() + PROMPT_TIMEOUT
        while self.alive and PROMPT not in self.since_enter:
            left = deadline - time.monotonic()
            if left <= 0:
                sys.exit(f"cast.py: no prompt within {PROMPT_TIMEOUT:g}s after: {what}")
            ready, _, _ = select.select([self.fd], [], [], left)
            if ready:
                self._read()
        if not self.alive:
            sys.exit(f"cast.py: the shell exited after: {what}")

    def send(self, data):
        os.write(self.fd, data.encode())

    def type_line(self, text):
        for ch in text:
            self.send(ch)
            self.pump(TYPING_DELAY)

    def enter(self):
        self.since_enter = bytearray()
        self.send("\r")

    # --- the session ---

    def preamble(self):
        # The shell's own first prompt, before anything is typed at it.
        self.pump(0.3)
        self.send(
            "export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null"
            " GIT_CONFIG_NOSYSTEM=1 GIT_EDITOR=false EDITOR=false FF_PAGER=cat"
            f" && cd {shell_quote(self.cwd)}"
            f" && PS1={shell_quote(PS1)} && unset PROMPT_COMMAND"
            " && bind 'set enable-bracketed-paste off' && clear\r"
        )
        self.wait_prompt("the preamble")
        # The clock starts on the prompt: the newline it opens with and
        # everything from the prompt on is the first event, and the clear
        # that came before it is not recorded because the cast opens on an
        # empty screen anyway.
        idx = self.since_enter.index(PROMPT)
        self.t0 = time.monotonic()
        self.events.append((0.0, self.decoder.decode(b"\r\n" + bytes(self.since_enter[idx:]))))

    def record(self, lines):
        self.pump(LEAD_IN)
        n = len(lines)
        for i, (tag, text) in enumerate(lines):
            self.type_line(text)
            self.pump(PAUSE[tag])
            self.enter()
            # A line with a continuation under it leaves the shell at its
            # secondary prompt: no primary prompt is coming until the last
            # line of the heredoc.
            if i + 1 < n and lines[i + 1][0] == "cont":
                self.pump(DWELL[tag])
                continue
            self.wait_prompt(text)
            self.pump(DWELL[tag])
        self.pump(TAIL)

    def finish(self):
        os.close(self.fd)
        try:
            os.waitpid(self.pid, 0)
        except ChildProcessError:
            pass
        header = {
            "version": 2,
            "width": self.cols,
            "height": self.rows,
            "timestamp": int(time.time()),
            "env": {"SHELL": "/bin/bash", "TERM": "xterm-256color"},
            "theme": THEME,
        }
        with open(self.out, "w", encoding="utf-8") as f:
            f.write(json.dumps(header, separators=(",", ":")) + "\n")
            for t, text in self.events:
                f.write(json.dumps([round(t, 6), "o", text], ensure_ascii=False, separators=(",", ":")) + "\n")


def shell_quote(s):
    return "'" + s.replace("'", "'\\''") + "'"


def parse_lines(stream):
    lines = []
    for raw in stream:
        raw = raw.rstrip("\n")
        if not raw:
            continue
        tag, _, text = raw.partition("|")
        if tag == "set":
            continue
        if tag not in PAUSE:
            sys.exit(f"cast.py: unknown tag {tag!r} in: {raw}")
        lines.append((tag, text))
    if not lines:
        sys.exit("cast.py: nothing to record")
    return lines


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--cwd", required=True, help="directory the recording runs in")
    ap.add_argument("--out", required=True, help="asciicast file to write")
    ap.add_argument("--cols", type=int, default=110)
    ap.add_argument("--rows", type=int, default=28)
    args = ap.parse_args()

    lines = parse_lines(sys.stdin)
    rec = Recorder(os.path.abspath(args.cwd), args.cols, args.rows, args.out)
    rec.preamble()
    rec.record(lines)
    rec.finish()


if __name__ == "__main__":
    main()
