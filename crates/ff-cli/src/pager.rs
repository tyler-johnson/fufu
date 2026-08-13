//! Paged output for the log family (`ff log`, `ff evolog`, `ff log --ops`),
//! git/jj-style: a pager spawns only when stdout is a real TTY and the view
//! is human (never `--json`), so pipes and scripts see plain direct bytes.
//! Pager choice: `fufu.pager` config, then `FF_PAGER`, then `PAGER`, then
//! `less` — whitespace-split, no shell quoting. `LESS=FRX` and
//! `LESSCHARSET=utf-8` are provided when unset (quit-if-one-screen, keep
//! ANSI colors, don't clear the screen). Any spawn failure falls back to
//! direct printing, silently.

use std::io::{IsTerminal, Write};
use std::process::{Child, Command, Stdio};

pub struct LogOut {
    inner: Inner,
    // expect lifted when the styled renderers land (evolog / change-centric log).
    #[expect(dead_code)]
    colored: bool,
}

enum Inner {
    Direct(anstream::AutoStream<std::io::Stdout>),
    Paged(Child),
}

impl LogOut {
    /// Decide color and destination for one log invocation. Color is decided
    /// against the real stdout BEFORE any pager wraps it — anstream's
    /// auto-detection (NO_COLOR, TERM=dumb, TTY) applies to the pager pipe
    /// too, since the pager just relays to the same terminal.
    pub fn new(repo: &ff_core::gix::Repository, json: bool) -> Self {
        let colored = !matches!(
            anstream::AutoStream::choice(&std::io::stdout()),
            anstream::ColorChoice::Never
        );
        if json || !std::io::stdout().is_terminal() {
            return Self::direct(colored);
        }
        let command = pager_command(repo);
        let mut parts = command.split_whitespace();
        let Some(program) = parts.next() else {
            return Self::direct(colored);
        };
        // An empty or `cat` pager means "no pager" — git's convention.
        if program == "cat" {
            return Self::direct(colored);
        }
        let mut cmd = Command::new(program);
        cmd.args(parts).stdin(Stdio::piped());
        if std::env::var_os("LESS").is_none() {
            cmd.env("LESS", "FRX");
        }
        if std::env::var_os("LESSCHARSET").is_none() {
            cmd.env("LESSCHARSET", "utf-8");
        }
        match cmd.spawn() {
            Ok(child) => LogOut {
                inner: Inner::Paged(child),
                colored,
            },
            Err(_) => Self::direct(colored),
        }
    }

    fn direct(colored: bool) -> Self {
        LogOut {
            inner: Inner::Direct(anstream::AutoStream::auto(std::io::stdout())),
            colored,
        }
    }

    /// Whether renderers should emit ANSI styling into this stream.
    #[expect(dead_code)]
    pub fn colored(&self) -> bool {
        self.colored
    }

    /// Drain and close: drop the pager's stdin and wait for it to exit (the
    /// user may still be scrolling). Must run before main's flush + exit.
    pub fn finish(self) {
        match self.inner {
            Inner::Direct(mut stream) => {
                let _ = stream.flush();
            }
            Inner::Paged(mut child) => {
                drop(child.stdin.take());
                let _ = child.wait();
            }
        }
    }
}

impl Write for LogOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let result = match &mut self.inner {
            Inner::Direct(stream) => stream.write(buf),
            Inner::Paged(child) => match child.stdin.as_mut() {
                Some(stdin) => stdin.write(buf),
                None => Ok(buf.len()),
            },
        };
        match result {
            // The user quit the pager mid-stream: pretend the write landed
            // and let the command finish quietly.
            Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(buf.len()),
            other => other,
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let result = match &mut self.inner {
            Inner::Direct(stream) => stream.flush(),
            Inner::Paged(child) => match child.stdin.as_mut() {
                Some(stdin) => stdin.flush(),
                None => Ok(()),
            },
        };
        match result {
            Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            other => other,
        }
    }
}

fn pager_command(repo: &ff_core::gix::Repository) -> String {
    if let Some(value) = repo.config_snapshot().string("fufu.pager") {
        return value.to_string();
    }
    std::env::var("FF_PAGER")
        .or_else(|_| std::env::var("PAGER"))
        .unwrap_or_else(|_| "less".into())
}
