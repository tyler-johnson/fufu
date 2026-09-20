//! `ff merge <branch>` — take a branch into the current one by one commit
//! with two parents, the current tip first; a fast-forward when this branch
//! has nothing of its own. The base is refused: `ff pull` and `ff restack`
//! replay onto it. A conflicting auto-merge holds, exit 3, for `ff resolve`,
//! or under `--resolve` and `fufu.onConflict resolve` holds and opens the
//! resolution session in the same run.

use ff_core::{MergeOutcome, Result};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    target: String,
    message: Option<String>,
    (resolve, no_resolve): (bool, bool),
) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (outcome, opened, verb_ctx) = ff_core::merge::merge(
        &repo,
        &target,
        message.as_deref(),
        crate::onconflict::settle(&repo, resolve, no_resolve),
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        MergeOutcome::Merged(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "merge": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("merge", &payload)?;
                if matches!(report.arrival, ff_core::ArrivalReport::Held { .. }) {
                    crate::exit::held();
                }
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            let short =
                crate::render::paint_sha(ff_core::sha::short(report.commit.as_str()), colored);
            if report.fast_forward {
                println!(
                    "fast-forwarded {} to {} at {short} — nothing of its own to merge",
                    report.branch, report.target
                );
            } else {
                println!("merged {} into {} at {short}", report.target, report.branch);
            }
            if report.files > 0 {
                println!("updated the working copy ({} file(s))", report.files);
            }
            // A held arrival exits 3 from inside the renderer.
            crate::cmd::switch::render_arrival(&report.arrival, &report.branch, colored);
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        MergeOutcome::Held(report) => {
            // With the session open the hold reads as one line and the
            // session's block follows, `ff resolve`'s own shape under the
            // merge door; the ways out are the session's.
            if ctx.json {
                let payload = match &opened {
                    Some(session) => serde_json::json!({
                        "merge": serde_json::Value::Null,
                        "held": report,
                        "resolve": session,
                        "undo": "ff undo",
                    }),
                    None => serde_json::json!({
                        "merge": serde_json::Value::Null,
                        "held": report,
                    }),
                };
                crate::machine::emit("merge", &payload)?;
                crate::exit::held();
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            match &opened {
                Some(session) => {
                    println!(
                        "{}",
                        crate::render::paint_warn(&crate::render::held_line(&report), colored)
                    );
                    super::resolve::render_opened(session, colored);
                }
                None => println!("{}", crate::render::held_block(&report, colored)),
            }
            crate::exit::held();
        }
    }
    Ok(())
}
