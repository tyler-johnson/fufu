//! `ff absorb` — fold the open change into a commit that has already
//! closed: the revision you name, or the one it sits on when you name
//! none. A path filter chooses which of the change's files fold in.

use ff_core::revset::{Rev, Revset};
use ff_core::{AbsorbOutcome, Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, into: Option<String>, paths: Vec<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // Absorb has no bare form: the open change is where your changes
    // already are, so naming it is a contradiction, not a target.
    let target = match &into {
        Some(src) => match Revset::parse(src)?.point(&repo)?.rev {
            Rev::Open => {
                return Err(Error::coded(
                    "usage/absorb-into-open",
                    "the open change is already where your changes are: name a commit that has closed",
                    vec!["ff absorb".into(), "ff commit -m <msg>".into()],
                ));
            }
            Rev::Commit(id) => Some(id.object_id()),
        },
        None => None,
    };

    let (outcome, verb_ctx) = ff_core::absorb::absorb(
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
        AbsorbOutcome::Absorbed(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "absorb": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("absorb", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            // `None` is the absorb dropping the commit it aimed at: there is
            // no new identity to name, so name the one it was.
            let short: String = report
                .new
                .as_deref()
                .unwrap_or(&report.into)
                .chars()
                .take(7)
                .collect();
            match &report.new {
                Some(_) => println!(
                    "absorbed into {}: {}",
                    crate::render::paint_sha(&short, colored),
                    report.subject
                ),
                None => println!(
                    "absorbed into {} \"{}\": the commit introduces nothing now and is gone",
                    crate::render::paint_sha(&short, colored),
                    report.subject
                ),
            }
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
            if let Some(line) =
                crate::render::dropped_line(&report.dropped, Some(&report.into), colored)
            {
                println!("{line}");
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
            if !report.paths.is_empty() {
                println!("limited to {} path(s)", report.paths.len());
            }
            if report.still_open {
                println!("the rest of your change is still open");
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        AbsorbOutcome::NothingToAbsorb { branch } => {
            if ctx.json {
                let payload = serde_json::json!({
                    "absorb": serde_json::Value::Null,
                    "branch": branch,
                    "nothing": true,
                });
                crate::machine::emit("absorb", &payload)?;
                return Ok(());
            }
            println!("nothing to absorb on {branch}");
        }
    }
    Ok(())
}
