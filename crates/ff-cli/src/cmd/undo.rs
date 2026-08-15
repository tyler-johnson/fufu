//! `ff undo` — roll the whole repository back to before an operation. Lean
//! by decree: no confirmation prompt (undo is itself one undo away), op ids
//! come from `ff log --ops`, redo = undo the undo.

use ff_core::{Result, UndoOptions};

pub fn run(op: Option<String>, force: bool, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
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
        let payload = serde_json::json!({
            "undo": report,
            "redo": "ff undo",
        });
        crate::machine::emit("undo", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    let label = if report.target_kind == "foreign" {
        " (a change made outside fufu)"
    } else {
        ""
    };
    println!(
        "undid {}{}: {}",
        crate::render::paint_sha(&report.target[..8], colored),
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
            (_, Some(new)) => {
                let sha = &new[..new.len().min(8)];
                format!("→ {}", crate::render::paint_sha(sha, colored))
            }
            (Some(_), None) => "deleted".to_string(),
            (None, None) => continue,
        };
        println!("  {} {what}", t.name);
    }
    if let Some(head) = &report.head_moved {
        let display = head.strip_prefix("ref:").unwrap_or(head);
        let is_sha = display.chars().all(|c| c.is_ascii_hexdigit());
        let painted = if is_sha {
            crate::render::paint_sha(display, colored)
        } else {
            display.to_string()
        };
        println!("  HEAD → {painted}");
    }
    if !report.files.is_empty() {
        println!("  {} worktree file(s) restored", report.files.len());
    }
    for warning in &report.warnings {
        println!(
            "{}",
            crate::render::paint_warn(&format!("  warning: {warning}"), colored)
        );
    }
    println!("{}", crate::render::paint_dim("redo: ff undo", colored));
    Ok(())
}
