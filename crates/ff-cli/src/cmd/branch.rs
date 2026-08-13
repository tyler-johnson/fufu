//! `ff branch` — list (named and anonymous segregated), claim, delete.

use ff_core::{BranchInfo, Error, Result};

pub fn run(name: Option<String>, delete: Option<String>, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    if let Some(target) = delete {
        let (report, ctx) = ff_core::branch::delete(
            &repo,
            &target,
            &crate::provenance::pre_ff(),
            None,
            std::env::args().collect(),
        )?;
        crate::render::reconcile_notice(&ctx.reconcile);
        if json {
            let body = serde_json::to_string(&serde_json::json!({
                "deleted": report,
                "undo": "ff undo",
            }))
            .map_err(Error::repo)?;
            println!("{body}");
            return Ok(());
        }
        println!(
            "deleted {} (was {})",
            report.name,
            &report.tip[..report.tip.len().min(8)]
        );
        if let Some(trash) = &report.trash_ref {
            println!("  its timeline moved to {trash}");
        }
        if report.parked_demoted.is_some() {
            println!("  its parked change stays in the stash (git stash list)");
        }
        println!("undo: ff undo");
        return Ok(());
    }
    if let Some(new_name) = name {
        let (report, ctx) = ff_core::branch::claim_current(
            &repo,
            &new_name,
            &crate::provenance::pre_ff(),
            None,
            std::env::args().collect(),
        )?;
        crate::render::reconcile_notice(&ctx.reconcile);
        if json {
            let body = serde_json::to_string(&serde_json::json!({
                "claimed": report,
                "undo": "ff undo",
            }))
            .map_err(Error::repo)?;
            println!("{body}");
            return Ok(());
        }
        println!("claimed {} as {}", report.from, report.to);
        println!("undo: ff undo");
        return Ok(());
    }

    // List. Reads don't reconcile-journal here; `ff status` owns loudness.
    let list = ff_core::branch::list(&repo)?;
    if json {
        let body = serde_json::to_string(&list).map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }
    for info in &list.named {
        println!("{}", row(info));
    }
    if !list.anonymous.is_empty() {
        if !list.named.is_empty() {
            println!();
        }
        println!("anonymous:");
        for info in &list.anonymous {
            println!("{}", row(info));
        }
    }
    Ok(())
}

fn row(info: &BranchInfo) -> String {
    let marker = if info.current { "*" } else { " " };
    let tip = info
        .tip
        .as_deref()
        .map(|t| &t[..t.len().min(7)])
        .unwrap_or("-------");
    let mut line = format!("{marker} {:<24} {tip}", info.name);
    if let Some(subject) = &info.subject {
        line.push_str(&format!("  {subject}"));
    }
    let mut notes = Vec::new();
    if info.parked {
        notes.push("parked change".to_string());
    }
    if let Some(desc) = &info.pending_description {
        notes.push(format!("pending: {desc}"));
    }
    if let Some(up) = &info.upstream {
        if up.gone {
            notes.push(format!("{} gone", up.r#ref));
        } else if up.ahead > 0 || up.behind > 0 {
            notes.push(format!("{}: +{} -{}", up.r#ref, up.ahead, up.behind));
        }
    }
    if !notes.is_empty() {
        line.push_str(&format!("  [{}]", notes.join(", ")));
    }
    line
}
