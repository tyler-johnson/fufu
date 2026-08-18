//! `ff commit` — close the open change. The pre-verb snapshot is mandatory
//! and owned by core; contention aborts before anything is written.

use ff_core::{CloseOptions, CommitOutcome, Result};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    message: Option<String>,
    no_verify: bool,
    branch: Option<String>,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    // The close's own mandatory pre-capture bypasses `capture::pre_best_effort`
    // (it's core's to take, not a generic capture-first read). The session
    // is now attached when `pre_ff()` builds the provenance, so every path
    // carries it without hand-wiring.
    let prov = crate::provenance::pre_ff(ctx);
    let (outcome, verb_ctx) = ff_core::close(
        &repo,
        &CloseOptions {
            message,
            no_verify,
            branch,
            now: None,
            argv: std::env::args().collect(),
        },
        &prov,
    )?;

    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "commit": outcome,
            "reconcile": verb_ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("commit", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    match &outcome {
        CommitOutcome::Closed {
            short_id,
            branch,
            subject,
            files_changed,
            claimed_from,
            ..
        } => {
            if let Some(old) = claimed_from {
                println!("claimed {old} as {branch}");
            }
            let described = if subject.is_empty() {
                "(no description)".to_string()
            } else {
                subject.clone()
            };
            println!(
                "closed {} on {}: {} ({} file(s))",
                crate::render::paint_sha(short_id, colored),
                branch,
                described,
                files_changed
            );
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
    }
    Ok(())
}
