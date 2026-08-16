//! `ff branch` — list (named and anonymous segregated), claim, delete.

use ff_core::{BranchInfo, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, name: Option<String>, delete: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    if let Some(target) = delete {
        let (report, verb_ctx) = ff_core::branch::delete(
            &repo,
            &target,
            &crate::provenance::pre_ff(ctx),
            None,
            std::env::args().collect(),
        )?;
        crate::render::init_palette(&repo);
        crate::render::reconcile_notice(&verb_ctx.reconcile);
        let colored = crate::pager::color_enabled();
        if ctx.json {
            let payload = serde_json::json!({
                "deleted": report,
                "undo": "ff undo",
            });
            crate::machine::emit("branch", &payload)?;
            return Ok(());
        }
        println!(
            "deleted {} (was {})",
            report.name,
            crate::render::paint_sha(&report.tip[..report.tip.len().min(8)], colored)
        );
        if let Some(trash) = &report.trash_ref {
            println!("  its timeline moved to {trash}");
        }
        if report.parked_demoted.is_some() {
            println!("  its parked change stays in the stash (git stash list)");
        }
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        return Ok(());
    }
    if let Some(new_name) = name {
        let (report, verb_ctx) = ff_core::branch::claim_current(
            &repo,
            &new_name,
            &crate::provenance::pre_ff(ctx),
            None,
            std::env::args().collect(),
        )?;
        crate::render::init_palette(&repo);
        crate::render::reconcile_notice(&verb_ctx.reconcile);
        let colored = crate::pager::color_enabled();
        if ctx.json {
            let payload = serde_json::json!({
                "claimed": report,
                "undo": "ff undo",
            });
            crate::machine::emit("branch", &payload)?;
            return Ok(());
        }
        println!("claimed {} as {}", report.from, report.to);
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        return Ok(());
    }

    // List. Reads don't reconcile here; `ff status` owns loudness.
    let list = ff_core::branch::list(&repo)?;
    if ctx.json {
        crate::machine::emit("branch", &list)?;
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
