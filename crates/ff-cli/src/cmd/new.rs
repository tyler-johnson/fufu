//! `ff new` — the composition verb: land the open change, open the next.

use ff_core::{ArrivalReport, CommitOutcome, Error, NewOptions, Result};

pub fn run(
    target: Option<String>,
    message: Option<String>,
    branch: Option<String>,
    json: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, ctx) = ff_core::new(
        &repo,
        &NewOptions {
            target,
            message,
            branch,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(),
    )?;

    crate::render::reconcile_notice(&ctx.reconcile);

    if json {
        let body = serde_json::to_string(&serde_json::json!({
            "new": report,
            "reconcile": ctx.reconcile,
            "undo": "ff undo",
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    if let Some(minted) = &report.minted {
        println!("minted {minted}");
    }
    if let Some(switch) = &report.switch {
        if let Some(stash) = &switch.parked {
            println!(
                "parked the open change on {} ({})",
                switch.from,
                &stash[..stash.len().min(8)]
            );
        }
        if switch.from != switch.to {
            println!("moved to {}", switch.to);
        }
        if let ArrivalReport::Restored { files, .. } = &switch.arrival {
            println!("resumed the parked change ({} file(s))", files.len());
        }
        if let ArrivalReport::StillParked { paths, .. } = &switch.arrival {
            println!("a parked change here no longer applies cleanly (stays parked):");
            for path in paths {
                println!("  conflicts: {path}");
            }
        }
    }
    match &report.commit {
        CommitOutcome::Closed {
            short_id,
            branch,
            subject,
            ..
        } => {
            let described = if subject.is_empty() {
                "(no description)".to_string()
            } else {
                subject.clone()
            };
            println!("closed {short_id} on {branch}: {described}");
        }
        CommitOutcome::NothingToClose { .. } => {}
    }
    println!("open change on {}", report.opened);
    println!("undo: ff undo");
    Ok(())
}
