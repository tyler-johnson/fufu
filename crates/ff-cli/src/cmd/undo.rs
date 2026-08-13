//! `ff undo` — roll the whole repository back to before an operation. Lean
//! by decree: no confirmation prompt (undo is itself one undo away), op ids
//! come from `ff log --ops`, redo = undo the undo.

use ff_core::{Error, Result, UndoOptions};

pub fn run(op: Option<String>, force: bool, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, ctx) = ff_core::undo(
        &repo,
        &UndoOptions {
            op,
            force,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(),
    )?;

    crate::render::reconcile_notice(&ctx.reconcile);

    if json {
        let body = serde_json::to_string(&serde_json::json!({
            "undo": report,
            "redo": "ff undo",
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    let label = if report.target_kind == "foreign" {
        " (a change made outside fufu)"
    } else {
        ""
    };
    println!(
        "undid {}{}: {}",
        &report.target[..8],
        label,
        report.target_summary
    );
    if report.rolled_back > 1 {
        println!(
            "  rolled back {} later operation(s) with it",
            report.rolled_back - 1
        );
    }
    for t in &report.refs {
        let what = match (&t.old, &t.new) {
            (_, Some(new)) => format!("→ {}", &new[..new.len().min(8)]),
            (Some(_), None) => "deleted".to_string(),
            (None, None) => continue,
        };
        println!("  {} {what}", t.name);
    }
    if let Some(head) = &report.head_moved {
        println!("  HEAD → {}", head.strip_prefix("ref:").unwrap_or(head));
    }
    if !report.files.is_empty() {
        println!("  {} worktree file(s) restored", report.files.len());
    }
    for warning in &report.warnings {
        println!("  warning: {warning}");
    }
    println!("redo: ff undo");
    Ok(())
}
