//! `ff switch` — branches without ceremony. Parking and arrival reports are
//! part of the verb's voice: the user should always know where their work
//! went and where it came back from.

use ff_core::{ArrivalReport, Result, SwitchOptions};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, target: String) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, verb_ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;

    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "switch": report,
            "reconcile": verb_ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("switch", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    if report.from == report.to {
        println!("already on {}", report.to);
        return Ok(());
    }
    if let Some(stash) = &report.parked {
        println!(
            "parked the open change on {} ({})",
            report.from,
            crate::render::paint_sha(&stash[..stash.len().min(8)], colored)
        );
    }
    println!("switched to {}", report.to);
    match &report.arrival {
        ArrivalReport::None => {}
        ArrivalReport::Restored { files, .. } => {
            println!("resumed the parked change ({} file(s))", files.len());
        }
        ArrivalReport::StillParked { paths, .. } => {
            println!("a parked change is waiting here but no longer applies cleanly:");
            for path in paths {
                println!(
                    "{}",
                    crate::render::paint_warn(&format!("  conflicts: {path}"), colored)
                );
            }
            println!("it stays parked; resolve by hand with git stash, or continue working");
        }
        ArrivalReport::Invalidated { stash } => {
            println!(
                "note: the parked change ({}) was dropped outside fufu; its entry was cleared",
                crate::render::paint_sha(&stash[..stash.len().min(8)], colored)
            );
        }
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}
