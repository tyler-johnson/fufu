//! `ff edit <rev>` opens an editing session on a commit: a branch minted at
//! the commit and switched to, so the commit's real content is what gets
//! edited, with the whole toolchain pointed at it. The branch you came from
//! stays exactly where it stands, its commits waiting ahead until `ff done`
//! amends the commit and replays them onto it.

use ff_core::{EditOutcome, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, rev: String) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (outcome, verb_ctx) = ff_core::edit::edit(
        &repo,
        &rev,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        EditOutcome::Opened(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "edit": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("edit", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            if let Some(stash) = &report.parked {
                println!(
                    "parked the open change on {} ({})",
                    report.onto,
                    crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
                );
            }
            println!(
                "editing {} \"{}\" on {}",
                crate::render::paint_sha(ff_core::sha::short(report.editing.as_str()), colored),
                report.subject,
                report.session
            );
            if report.ahead > 0 {
                println!("{} commit(s) wait ahead on {}", report.ahead, report.onto);
            }
            println!(
                "{}",
                crate::render::paint_dim(
                    "finish with ff done, or ff done --abandon to drop it",
                    colored
                )
            );
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        EditOutcome::Switched(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "edit": serde_json::Value::Null,
                    "switched": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("edit", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            println!("that is a branch, not a commit — switching instead");
            crate::cmd::switch::render_switch(&report, colored);
        }
    }
    Ok(())
}
