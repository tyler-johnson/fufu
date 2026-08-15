use crate::render::ForeignChanges;
use ff_core::{LogEntry, Result};

pub fn run(json: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    run_inner(json)
}

/// The verb itself, capture already handled (or deliberately skipped).
pub fn run_inner(json: bool) -> Result<()> {
    let mut repo = ff_core::discover(".")?;

    let status = ff_core::status(&repo)?;
    let open = ff_core::open_change(&repo)?;
    let change_stat = ff_core::change_stat(&repo)?;

    // Parent commit: fetch at most 1 log entry
    let parent: Option<LogEntry> =
        ff_core::log(&mut repo, &ff_core::LogOptions { limit: Some(1) })?
            .next()
            .transpose()?;

    // Lens map: open change id + parent id for display
    let mut ids: Vec<String> = Vec::new();
    if let Some(id) = &open.id {
        ids.push(id.clone());
    }
    if let Some(ref p) = parent {
        ids.push(p.id.clone());
    }
    let lens = crate::cmd::evolog::displayed_prefix_lens(&repo, &ids)?;

    // Reconcile pinned (foreign changes)
    let foreign = reconcile_foreign(&repo);

    let now = now_secs();
    let colored = crate::pager::color_enabled();

    if json {
        render_json(&status, &open, &change_stat, &parent, colored)?;
    } else {
        crate::render::init_palette(&repo);
        let view = crate::render::StatusView {
            status: &status,
            open: &open,
            change_stat: &change_stat,
            lens: &lens,
            parent: parent.as_ref(),
            now,
            colored,
            foreign,
        };
        print!("{}", crate::render::status_human(&view));
    }

    Ok(())
}

fn render_json(
    status: &ff_core::Status,
    open: &ff_core::OpenChange,
    change_stat: &ff_core::ChangeStat,
    parent: &Option<LogEntry>,
    _colored: bool,
) -> Result<()> {
    let payload = serde_json::json!({
        "head": status.head,
        "operation": status.operation,
        "upstream": status.upstream,
        "changes": change_stat.files,
        "insertions": change_stat.insertions,
        "deletions": change_stat.deletions,
        "open": {
            "id": open.id,
            "id_letters": open.id.as_deref().map(ff_core::snapid::encode),
            "pending": open.pending,
            "subject": open.subject,
            "clean": open.clean,
        },
        "parent": parent.as_ref().map(|p| serde_json::json!({
            "id": p.id,
            "subject": p.subject,
        })),
        "conflicts": status.conflicts,
    });
    crate::machine::emit("status", &payload)
}

/// Reconcile (best-effort — status must never fail because the journal
/// can't be written) and return foreign ref changes while the tip is foreign.
fn reconcile_foreign(repo: &ff_core::gix::Repository) -> ForeignChanges {
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
    (entry.record.kind == ff_core::journal::OpKind::Foreign).then(|| {
        entry
            .record
            .refs
            .into_iter()
            .map(|t| (t.name, t.old, t.new))
            .collect()
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
