//! Capture-first: the pre-command capture every ff command takes before its
//! own work. Best-effort by design — a read command on a broken capture layer
//! must behave byte-identically to one without a capture layer at all.

use ff_core::Provenance;

/// Take the pre-command capture against an already-discovered repository,
/// swallowing every failure (diagnostics only under `FF_DEBUG=1`). In a bare
/// repository this is a silent no-op. Never touches exit codes or stdout.
///
/// One thing does reach stderr: a capture's warnings. The contract this
/// weakens — a read command behaves byte-identically to one with no capture
/// layer — was written against *failures*, which are nobody's business on a
/// read. A warning here reports that pre-cutover refs were parked rather than
/// overwritten, it can only happen once in a repository's life, and a read is
/// a perfectly likely first command after an upgrade. Silence there would
/// make the receipt worthless precisely when it is owed.
pub fn pre_best_effort_in(repo: &ff_core::gix::Repository, prov: &Provenance) {
    match pre_in(repo, prov) {
        Ok(warnings) => {
            for warning in warnings {
                eprintln!("ff: {warning}");
            }
        }
        Err(err) if std::env::var_os("FF_DEBUG").is_some() => {
            eprintln!("ff[debug]: pre-capture failed: {err}");
        }
        Err(_) => {}
    }
}

/// The loud variant for `ff git`: a real capture failure prints a notice to
/// stderr, and the command proceeds anyway — capture must never block git.
pub fn pre_loud(prov: &Provenance) {
    let colored = crate::pager::color_enabled();
    match pre(prov) {
        Ok(warnings) => {
            for warning in warnings {
                eprintln!("ff: {warning}");
            }
        }
        Err(err) => eprintln!(
            "{}",
            crate::render::paint_warn(&format!("ff: capture skipped: {err}"), colored)
        ),
    }
}

/// Discover the repository, then take the capture's warnings against it.
fn pre(prov: &Provenance) -> ff_core::Result<Vec<String>> {
    match ff_core::discover(".") {
        Ok(repo) => pre_in(&repo, prov),
        // Not in a repository: nothing to capture, nothing to say.
        Err(ff_core::Error::Discover(_)) => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// The capture's warnings, if it had any. Empty is the overwhelmingly common
/// answer — the only warnings a capture raises are one-time.
fn pre_in(repo: &ff_core::gix::Repository, prov: &Provenance) -> ff_core::Result<Vec<String>> {
    if repo.workdir().is_none() {
        return Ok(Vec::new());
    }
    // Contended is a fact, not a failure: someone else is capturing right now.
    Ok(match ff_core::capture(repo, prov)? {
        ff_core::CaptureOutcome::Created { warnings, .. }
        | ff_core::CaptureOutcome::NoOp { warnings, .. } => warnings,
        ff_core::CaptureOutcome::Contended => Vec::new(),
    })
}
