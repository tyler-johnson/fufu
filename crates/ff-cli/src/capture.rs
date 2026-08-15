//! Capture-first: the pre-command snapshot every ff command takes before its
//! own work. Best-effort by design — a read command on a broken capture layer
//! must behave byte-identically to one without a capture layer at all.

use ff_core::Provenance;

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
    // Contended is a fact, not a failure: someone else is capturing right now.
    ff_core::take(&repo, prov)?;
    Ok(())
}
