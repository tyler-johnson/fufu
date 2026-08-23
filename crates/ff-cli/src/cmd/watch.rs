//! `ff watch` — the operation log gets subscribers.
//!
//! The log is already an event log; this is the verb that lets something
//! else read it as one. Status lines, editor plugins, dashboards and agent
//! supervisors want to know when the repository moved, and the alternative
//! they have today is polling `ff log --json` in a loop.
//!
//! **It is not a daemon.** It is a foreground process the user started, it
//! holds no authority, and it writes nothing — no capture, no reconcile, no
//! ref. `ff_core::watch::classify` reads two refs and a chain of message
//! trailers; everything here is the process around that: a flag surface, a
//! timer, and a line per motion.
//!
//! One caveat rides every event, inherited rather than introduced.
//! Operations are written **write-ahead**, before the mutation they
//! describe, so an operation reported here is a claim about the immediate
//! future rather than an observation of the past. `ff op log` shows the same
//! operation with the same caveat, and the mutation follows within
//! microseconds — well inside one poll interval. It is worth knowing and it
//! is not worth a field.

use ff_core::gix;
use ff_core::ops::OpLog;
use ff_core::watch::{self, Filter, Motion};
use ff_core::{Error, Result};

use crate::ctx::Ctx;

/// How often the log's tip is re-read, in milliseconds, when
/// `fufu.watchInterval` says nothing. One tick is a `try_find_reference` on
/// each of two refs — an open-and-read of about forty bytes, with gix
/// holding the packed-refs buffer — so this is sub-millisecond work five
/// times a second.
const DEFAULT_INTERVAL_MS: u64 = 200;

/// A floor under the configured interval. Zero would spin a core at a
/// hundred percent to answer a question the log cannot possibly re-answer
/// that fast, and a knob that lets a foreground process do that is a knob
/// with a bug in it.
const MIN_INTERVAL_MS: u64 = 10;

pub fn run(
    // Nothing on the context applies: this verb declares no `--at-op`, and
    // `--json` is not offered because the output is always JSON.
    _ctx: &Ctx,
    since: Option<String>,
    kind: Option<String>,
    session: Option<String>,
    count: Option<usize>,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    // The core owns the kind vocabulary and its error, so an unknown value
    // is refused here with the same words `--kind` would get anywhere else.
    let filter = Filter::new(kind.as_deref(), session)?;

    // `resolve` accepts `@`, a letters-spelled id or prefix, and git's
    // first-parent suffixes; `live` is the check that raises `op/trimmed`
    // when retention has already aged the anchor off the log.
    let since = match since {
        None => None,
        Some(spec) => {
            let log = OpLog::open(&repo)?;
            let id = log.resolve(&spec)?;
            log.live(id)?;
            Some(id)
        }
    };

    // Unbounded and `0` are the same wish. Every emitted event counts,
    // `start` included, which is what makes `-n 1` an anchor and nothing
    // else — and what lets a test bound this verb without a clock.
    let mut left = match count {
        None | Some(0) => usize::MAX,
        Some(n) => n,
    };

    // A locked handle for the life of the stream: a subscriber on a pipe
    // must not wait on a block buffer, and re-locking per line would pay for
    // the lock five times a second to no purpose.
    let mut out = std::io::stdout().lock();

    // Read both anchors through the same call the loop uses, rather than
    // reading the refs again here: a second read would describe a different
    // instant than the one the first motion is measured against.
    let seed = watch::classify(&repo, None, None, &filter)?;
    let mut last_seen = since.or(seed.tip);
    let mut last_trash = seed.trash;

    // Always first, so a subscriber holds an id before anything streams. It
    // names where this stream begins — the `--since` operation when one was
    // given, the live tip otherwise.
    if !emit(&mut out, &Motion::Start { tip: last_seen })? {
        return Ok(());
    }
    left -= 1;
    if left == 0 {
        return Ok(());
    }

    let interval = std::time::Duration::from_millis(interval_ms(&repo));
    loop {
        // Sleep first: a `--since` replay must not race the anchor read that
        // just settled above.
        std::thread::sleep(interval);

        let seen = watch::classify(&repo, last_seen, last_trash, &filter)?;
        last_seen = seen.tip;
        last_trash = seen.trash;

        for motion in &seen.motion {
            if !emit(&mut out, motion)? {
                return Ok(());
            }
            // Terminal. Every id the subscriber holds stops resolving at a
            // rewrite, so this is the end of a stream rather than an event
            // within one, and the shell is told so it can reconnect.
            if matches!(motion, Motion::Rewritten { .. }) {
                crate::exit::rewritten();
                return Ok(());
            }
            left -= 1;
            if left == 0 {
                return Ok(());
            }
        }
    }
}

/// Write one envelope and flush it. `false` means the far end went away.
///
/// **`ff watch | head -1` closes the pipe under a live writer.** Rust never
/// restores the default SIGPIPE disposition, so the process is not killed by
/// the signal — the write returns `EPIPE`, and `println!` *panics* on a
/// write error. A panic at the end of a pipeline is a bug report, so a
/// broken pipe is a clean exit 0 here: no error line, no stack trace.
///
/// The pipe is noticed on the next write and not before, so a stream with
/// nothing to say sits in its poll loop until something moves. That is what
/// `tail -f | head -1` does too, and for the same reason: learning that a
/// reader has gone means writing to it.
///
/// The envelope is built by [`crate::machine::write`] so the framing lives
/// in exactly one place, but it is built into a buffer rather than onto
/// stdout: `machine::write` maps its io error through `Error::repo`, which
/// keeps the message and drops the `ErrorKind`, and the kind is the whole
/// question being asked here.
fn emit<W: std::io::Write>(out: &mut W, motion: &Motion) -> Result<bool> {
    let mut line = Vec::new();
    crate::machine::write(&mut line, "watch", motion)?;
    match out.write_all(&line).and_then(|()| out.flush()) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(err) => Err(Error::repo(err)),
    }
}

fn interval_ms(repo: &gix::Repository) -> u64 {
    repo.config_snapshot()
        .integer("fufu.watchInterval")
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(DEFAULT_INTERVAL_MS)
        .max(MIN_INTERVAL_MS)
}
