//! `ff start` — always begins a new line of work on a fresh branch. No
//! invocation produces a commit.

use ff_core::{Result, StartOptions};

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

    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&ctx.reconcile);

    if json {
        let payload = serde_json::json!({
            "start": report,
            "reconcile": ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("start", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    if let Some(stash) = &report.parked {
        println!(
            "parked the open change on {} ({})",
            report.forked_from,
            crate::render::paint_sha(&stash[..stash.len().min(8)], colored)
        );
    }
    println!(
        "minted {} (forked from {})",
        report.minted, report.forked_from
    );
    println!("open change on {}", report.minted);
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}
