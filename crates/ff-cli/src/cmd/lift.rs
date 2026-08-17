//! `ff lift` — the other direction of absorb: take paths out of a commit
//! that has already closed and back into the open change, the revision you
//! name or the one it sits on when you name none.

use ff_core::revset::{Rev, Revset};
use ff_core::{Error, LiftOutcome, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, from: Option<String>, paths: Vec<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // Lift has no bare form either: the open change is what a lift
    // lands in, so naming it as a source has nothing to lift out of.
    let target = match &from {
        Some(src) => match Revset::parse(src)?.point(&repo)?.rev {
            Rev::Open => {
                return Err(Error::coded(
                    "usage/lift-from-open",
                    "the open change has nothing committed to lift out of: name a commit that has closed",
                    vec!["ff lift".into(), "ff log".into()],
                ));
            }
            Rev::Commit(id) => Some(id.object_id()),
        },
        None => None,
    };

    let (outcome, verb_ctx) = ff_core::absorb::lift(
        &repo,
        target,
        paths,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        LiftOutcome::Lifted(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "lift": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("lift", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            let short: String = report.new.chars().take(7).collect();
            println!(
                "lifted out of {}: {}",
                crate::render::paint_sha(&short, colored),
                report.subject
            );
            if report.restacked > 0 {
                if report.moved.is_empty() {
                    println!("restacked {} commit(s) above it", report.restacked);
                } else {
                    println!(
                        "restacked {} commit(s) above it; moved {}",
                        report.restacked,
                        report.moved.join(", ")
                    );
                }
            }
            if report.emptied {
                println!("that commit is empty now");
            }
            if report.published > 0 {
                // Disclosure, not a warning, on the same rule as reword.
                let upstream_name = ff_core::upstream(&repo)?
                    .map(|u| u.r#ref)
                    .unwrap_or_else(|| "the remote".to_string());
                println!(
                    "{} of the rewritten commits are already on {}",
                    report.published, upstream_name
                );
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        LiftOutcome::NothingToLift { from } => {
            if ctx.json {
                let payload = serde_json::json!({
                    "lift": serde_json::Value::Null,
                    "from": from,
                    "nothing": true,
                });
                crate::machine::emit("lift", &payload)?;
                return Ok(());
            }
            let short: String = from.chars().take(7).collect();
            println!("nothing to lift out of {short}");
        }
    }
    Ok(())
}
