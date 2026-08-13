use ff_core::{Error, Result};

pub fn run(json: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    run_inner(json)
}

/// The verb itself, capture already handled (or deliberately skipped).
/// Status is where reconciliation loudness stays pinned: foreign motion is
/// absorbed here (lazily, like every invocation) and keeps being shown as
/// long as the journal tip is a foreign entry — until the next fufu op.
pub fn run_inner(json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let status = ff_core::status(&repo)?;
    let pinned = reconcile_pinned(&repo);
    if json {
        // The Phase-1 status shape survives unchanged; the journal view
        // rides alongside only when there is something to say.
        let body = match &pinned {
            Some(entry) => serde_json::to_string(&serde_json::json!({
                "head": status.head,
                "operation": status.operation,
                "upstream": status.upstream,
                "staged": status.staged,
                "unstaged": status.unstaged,
                "untracked": status.untracked,
                "conflicts": status.conflicts,
                "foreign": entry.record.refs,
            })),
            None => serde_json::to_string(&status),
        }
        .map_err(Error::repo)?;
        println!("{body}");
    } else {
        print!("{}", crate::render::status_human(&status));
        if let Some(entry) = &pinned {
            println!("changes made outside fufu (absorbed; ff undo can roll them back):");
            for t in &entry.record.refs {
                let what = match (&t.old, &t.new) {
                    (_, Some(new)) => format!("moved to {}", &new[..new.len().min(8)]),
                    (Some(_), None) => "deleted".to_string(),
                    (None, None) => continue,
                };
                println!("  {} {what}", t.name);
            }
        }
    }
    Ok(())
}

/// Reconcile (best-effort — status must never fail because the journal
/// can't be written) and return the tip entry while it is foreign.
fn reconcile_pinned(repo: &ff_core::gix::Repository) -> Option<ff_core::journal::Entry> {
    repo.workdir()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match ff_core::journal::reconcile(repo, now) {
        Ok(report) => {
            for warning in &report.warnings {
                eprintln!("ff: {warning}");
            }
        }
        Err(err) => {
            if std::env::var_os("FF_DEBUG").is_some() {
                eprintln!("ff[debug]: reconcile failed: {err}");
            }
            return None;
        }
    }
    let tip = ff_core::journal::tip(repo).ok().flatten()?;
    let entry = ff_core::journal::read_entry(repo, tip).ok()?;
    (entry.record.kind == ff_core::journal::OpKind::Foreign).then_some(entry)
}
