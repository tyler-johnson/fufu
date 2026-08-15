//! Capture-first: the pre-command snapshot every ff command takes before its
//! own work. Best-effort by design — a read command on a broken capture layer
//! must behave byte-identically to one without a capture layer at all.

use ff_core::Provenance;
use ff_core::gix;

/// Take the pre-command snapshot, swallowing every failure (diagnostics only
/// under `FF_DEBUG=1`). Outside a repository or in a bare one this is a
/// silent no-op. Never touches exit codes, stdout, or stderr.
pub fn pre_best_effort(prov: &Provenance) {
    if let Err(err) = pre(prov)
        && std::env::var_os("FF_DEBUG").is_some()
    {
        eprintln!("ff[debug]: pre-capture failed: {err}");
    }
}

/// The loud variant for `ff git`: a real capture failure prints a notice to
/// stderr, and the command proceeds anyway — capture must never block git.
pub fn pre_loud(prov: &Provenance) {
    if let Err(err) = pre(prov) {
        let colored = crate::pager::color_enabled();
        eprintln!(
            "{}",
            crate::render::paint_warn(&format!("ff: snapshot skipped: {err}"), colored)
        );
    }
}

fn pre(prov: &Provenance) -> ff_core::Result<()> {
    let repo = match ff_core::discover(".") {
        Ok(repo) => repo,
        // Not in a repository: nothing to capture, nothing to say.
        Err(ff_core::Error::Discover(_)) => return Ok(()),
        Err(err) => return Err(err),
    };
    if repo.workdir().is_none() {
        return Ok(());
    }
    // Resolve the current session and attach it to the provenance.
    let prov = attach_session(&repo, prov);
    // Contended is a fact, not a failure: someone else is capturing right now.
    ff_core::take(&repo, &prov)?;
    Ok(())
}

/// Resolve the current session and attach it to the provenance. Core must
/// not read `FF_SESSION` itself — resolution is the CLI's job.
///
/// `pub(crate)`: `pre_best_effort`/`pre_loud` call this for every
/// capture-first read command, but a verb whose own mandatory pre-capture
/// bypasses this module entirely — `ff commit`'s `ff_core::close`, in
/// particular — needs the same resolution before building its own
/// `Provenance`, or a commit made while a session is open would close on a
/// pre-capture with no session trailer at all, and `ff log --session` would
/// never find it.
pub(crate) fn attach_session(repo: &gix::Repository, prov: &Provenance) -> Provenance {
    let session = crate::session::current(repo).map(|m| m.name);
    prov.clone().with_session(session)
}
