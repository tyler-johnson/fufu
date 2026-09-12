//! The ambient lanes in one place: what rides an invocation besides the verb
//! itself — the pre-command capture, the passive update lane, the daily
//! auto-trim, and the fetch on `fufu.autoFetch`'s cadence. A verb is in the
//! table by construction rather than by somebody remembering to call four
//! functions at the bottom of its `run`.

/// The before half: ride the lanes the table says this invocation carries,
/// and hand back the one discovered repository the trailer needs.
///
/// `None` means the lanes paid nothing — either the table gave the verb
/// none, or there is no repository for them to ride.
pub fn preflight(
    ctx: &crate::ctx::Ctx,
    lanes: &crate::cli::Lanes,
) -> Option<ff_core::gix::Repository> {
    // No lane on: the foreign redirects, `update`, `init`, `clone`, `git`
    // and `hook` pay nothing at all — not even a discover.
    if !lanes.capture
        && !lanes.update
        && !lanes.trim
        && !lanes.notice
        && lanes.fetch == crate::cli::Fetch::Off
    {
        return None;
    }
    // A lane must never change what a command does, so discovery here is
    // best-effort: outside a repository the answer is `None` and every lane
    // is skipped, never an error. One discover serves all the lanes and is
    // then handed to the trailer, where before this module each lane that
    // ran discovered for itself.
    //
    // The fetch lane is the one exception to the rule's letter, and on
    // purpose: it changes what a command *reads*, which is its whole point.
    // What it keeps is the rule's substance — no outcome of the fetch turns
    // the verb's exit into a failure. `maybe_fetch` consumes its own
    // `Result`, and a remote that is down costs the verb a bounded wait and
    // one dim line, never its answer.
    let repo = ff_core::discover(".").ok()?;
    if lanes.capture {
        crate::capture::pre_best_effort_in(&repo, &crate::provenance::pre_ff(ctx));
    }
    if lanes.update {
        crate::selfupdate::notify::maybe_spawn_check(&repo);
    }
    if lanes.fetch != crate::cli::Fetch::Off {
        crate::autofetch::maybe_fetch(&repo, ctx, lanes.fetch);
    }
    Some(repo)
}

/// The after half: the lanes that ride a successful invocation, in their
/// order. Skipped entirely when preflight found no repository.
pub fn trailer(lanes: &crate::cli::Lanes, repo: Option<&ff_core::gix::Repository>) {
    let Some(repo) = repo else {
        return;
    };
    if lanes.trim {
        crate::autotrim::maybe_trim(repo);
    }
    if lanes.update {
        // Both halves of the passive lane live in `pending`: the auto-install
        // always fires, and `notice` decides only whether this invocation is
        // handed a line to say. A release announces at most once, ever, so
        // a line that is printed is marked spent in the same breath.
        let notices =
            crate::selfupdate::notify::pending(repo, env!("CARGO_PKG_VERSION"), lanes.notice);
        if !notices.is_empty() {
            for notice in &notices {
                eprintln!("{}", notice.line);
            }
            crate::selfupdate::notify::mark_notified(&notices);
        }
    }
}
