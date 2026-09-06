//! `ff resolve` — deal with a held rewrite. It materializes the conflicts
//! all at once on a session branch, the way `ff edit` opens one, and
//! switches you there; `--abandon` drops the hold instead. Unlike a hold,
//! none of its outcomes is a refusal: a resolution that opened is a
//! success, so nothing here sets an exit code.

use ff_core::{ResolveOutcome, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, abandon: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (outcome, verb_ctx) = ff_core::resolve::resolve(
        &repo,
        abandon,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        ResolveOutcome::Opened(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "resolve": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("resolve", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            if let Some(stash) = &report.parked {
                println!(
                    "parked the open change on {} ({})",
                    report.branch,
                    crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
                );
            }
            println!(
                "resolving {} conflict{} in {} on {}",
                report.regions,
                if report.regions == 1 { "" } else { "s" },
                report.files.join(", "),
                report.session
            );
            match &report.tangled {
                Some(subject) => println!(
                    "    {} of {} commits replayed; the rest waits on \"{}\"",
                    report.steps, report.of, subject
                ),
                None => println!("    {} commits replayed", report.of),
            }
            println!(
                "    {}",
                crate::render::paint_dim(
                    "fix the markers, then ff done · ff resolve --abandon to drop it",
                    colored
                )
            );
        }
        ResolveOutcome::Released(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "resolve": serde_json::Value::Null,
                    "released": report,
                });
                crate::machine::emit("resolve", &payload)?;
                return Ok(());
            }
            println!(
                "the rewrite is clean now: the hold is released, and re-running ff {} will land it",
                report.verb
            );
        }
        ResolveOutcome::Abandoned(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "resolve": serde_json::Value::Null,
                    "abandoned": report,
                });
                crate::machine::emit("resolve", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            match &report.session {
                Some(session) => println!(
                    "dropped the held {} on {} and the session {} with it",
                    report.verb, report.branch, session
                ),
                None if report.was_resolving => println!(
                    "dropped the held {} on {} and the resolution with it",
                    report.verb, report.branch
                ),
                None => println!("dropped the held {} on {}", report.verb, report.branch),
            }
            // HEAD moved only when the abandon ran from the session; a
            // session deleted from the held branch leaves HEAD where it stood.
            if report.returned {
                println!("back on {}", report.branch);
                crate::cmd::switch::render_arrival(&report.arrival, colored);
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
    }
    Ok(())
}
