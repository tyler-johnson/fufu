//! `ff restore` — pull worktree state back from the timeline. No best-effort
//! pre-capture here: restore's own pre-snapshot is mandatory, and its
//! failure aborts the restore.

use ff_core::{Error, RestoreOptions, Result};

pub fn run(at: Option<String>, all: bool, paths: Vec<String>, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let report = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: at,
            paths,
            all,
            now: None,
        },
        &crate::provenance::pre_ff(),
    )?;

    if json {
        let body = serde_json::to_string(&serde_json::json!({
            "target": report.target,
            "restored": report.restored,
            "deleted": report.deleted,
            "skipped_gitlinks": report.skipped_gitlinks,
            "pre_snapshot": report.pre_snapshot,
            "undo": "ff restore --all",
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    println!(
        "restored to {} ({})",
        report.target.short_id, report.target.subject
    );
    for path in &report.restored {
        println!("  restored  {path}");
    }
    for path in &report.deleted {
        println!("  deleted   {path}");
    }
    for path in &report.skipped_gitlinks {
        println!("  skipped   {path} (embedded repository)");
    }
    if report.restored.is_empty() && report.deleted.is_empty() {
        println!("  (no files differed)");
    }
    println!(
        "{}",
        crate::render::paint_dim("undo: ff restore --all", colored)
    );
    Ok(())
}
