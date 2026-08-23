//! The exit code a *successful* command still owes the shell.
//!
//! A held rewrite is an outcome and not an error: something happened, it has
//! a report, and the report goes to stdout like any other. But nothing moved
//! and a human decision is required, which is exactly what exit 3 means — so
//! the command succeeds and the process still exits 3.
//!
//! `main` has one `exit` at its tail and thirty verbs returning `Result<()>`.
//! Threading a code back through all of them would cost every verb a wider
//! signature to serve the four that can ever say this, so the code is set
//! where the outcome is rendered and read once at the end. It is written at
//! most once per process, by the same thread that is about to return.

use std::sync::atomic::{AtomicI32, Ordering};

static CODE: AtomicI32 = AtomicI32::new(0);

/// A rewrite held: nothing moved, and a human decision is required.
pub(crate) fn held() {
    CODE.store(3, Ordering::Relaxed);
}

/// A watch ended because the log was rewritten under it. Same shape as a
/// held rewrite: an outcome, not an error. It has a report, the report went
/// to stdout like every other line the stream wrote, and an error envelope
/// after it would contradict the line it just emitted — but the shell still
/// has to be told, because every id the subscriber holds has stopped
/// resolving and it must reconnect rather than carry on.
pub(crate) fn rewritten() {
    CODE.store(1, Ordering::Relaxed);
}

/// The code `main` exits with when no verb returned an error.
pub(crate) fn code() -> i32 {
    CODE.load(Ordering::Relaxed)
}
