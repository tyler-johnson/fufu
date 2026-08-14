//! `ff start` — always begins a new line of work on a fresh branch. No
//! invocation produces a commit.

use ff_core::{Error, Result, StartOptions};

pub fn run(
    target: Option<String>,
    message: Option<String>,
    branch: Option<String>,
    json: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, ctx) = ff_core::start(
        &repo,
        &StartOptions {
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
            "start": report,
            "reconcile": ctx.reconcile,
            "undo": "ff undo",
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    if let Some(stash) = &report.parked {
        println!(
            "parked the open change on {} ({})",
            report.forked_from,
            &stash[..stash.len().min(8)]
        );
    }
    println!(
        "minted {} (forked from {})",
        report.minted, report.forked_from
    );
    println!("open change on {}", report.minted);
    println!("undo: ff undo");
    Ok(())
}
