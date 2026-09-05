//! `ff restack` — replay a branch's commits onto the base it sits on: the
//! branch it was forked from when one was recorded, trunk otherwise. The
//! positional names the branch being moved; `--onto` names where it lands,
//! recorded as its new parent. A branch you are not standing on costs no
//! file on disk.

use ff_core::{RestackOutcome, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, branch: Option<String>, onto: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (outcome, verb_ctx) = ff_core::restack::restack(
        &repo,
        branch,
        onto,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        RestackOutcome::Restacked(report) => {
            // A branch above that held needs a person before anything more
            // moves there: the same 3 a hold on the branch itself owes the
            // shell, with the landed report still on stdout.
            let held_above = !report.cascade.held.is_empty();
            if ctx.json {
                let payload = serde_json::json!({
                    "restack": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("restack", &payload)?;
                if held_above {
                    crate::exit::held();
                }
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            if report.reaimed {
                match &report.previous_parent {
                    Some(was) => println!(
                        "re-aimed {} at {} (was {})",
                        report.branch, report.base, was
                    ),
                    None => println!("re-aimed {} at {}", report.branch, report.base),
                }
            } else if report.behind > 0 {
                println!("{} moved ahead by {} commit(s)", report.base, report.behind);
            }
            if report.fast_forward {
                println!(
                    "fast-forwarded {} to {} — nothing to replay",
                    report.branch, report.base
                );
            }
            if report.replayed > 0 {
                if report.moved.is_empty() {
                    println!(
                        "replayed {} commit(s) onto {}",
                        report.replayed, report.base
                    );
                } else {
                    println!(
                        "replayed {} commit(s) onto {}; moved {}",
                        report.replayed,
                        report.base,
                        report.moved.join(", ")
                    );
                }
            }
            // Divergence is a warning rather than a disclosure: something the
            // user did not ask for is now true of a branch they were not
            // thinking about.
            if !report.diverged.is_empty() {
                let sits = if report.diverged.len() == 1 {
                    "sits"
                } else {
                    "sit"
                };
                println!(
                    "{}",
                    crate::render::paint_warn(
                        &format!(
                            "{} now {sits} on commits this restack replaced",
                            report.diverged.join(", ")
                        ),
                        colored
                    )
                );
            }
            if let Some(line) = crate::render::dropped_line(&report.dropped, None, colored) {
                println!("{line}");
            }
            if report.files > 0 {
                if report.still_open {
                    println!(
                        "updated the working copy ({} file(s)); your change is still open",
                        report.files
                    );
                } else {
                    println!("updated the working copy ({} file(s))", report.files);
                }
            }
            if let Some(parked) = &report.parked {
                if parked.applies {
                    println!("that branch has a parked change — it still applies cleanly");
                } else {
                    println!(
                        "{}",
                        crate::render::paint_warn(
                            "that branch has a parked change — it would conflict on arrival",
                            colored
                        )
                    );
                }
            }
            if report.published > 0 {
                // Disclosure, not a warning, on the same rule as reword — and
                // read off the report rather than asked of HEAD, because
                // restack can name a branch you are not standing on.
                let upstream_name = report.published_on.as_deref().unwrap_or("the remote");
                println!(
                    "{} of the rewritten commits are already on {}",
                    report.published, upstream_name
                );
            }
            for line in crate::render::cascade_lines(&report.cascade, colored) {
                println!("{line}");
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
            if held_above {
                crate::exit::held();
            }
        }
        RestackOutcome::Held(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "restack": serde_json::Value::Null,
                    "held": report,
                });
                crate::machine::emit("restack", &payload)?;
                crate::exit::held();
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            println!("{}", crate::render::held_block(&report, colored));
            crate::exit::held();
        }
        RestackOutcome::NothingToRestack { branch, base } => {
            if ctx.json {
                let payload = serde_json::json!({
                    "restack": serde_json::Value::Null,
                    "branch": branch,
                    "base": base,
                    "nothing": true,
                });
                crate::machine::emit("restack", &payload)?;
                return Ok(());
            }
            println!("{branch} is already on top of {base}");
        }
    }
    Ok(())
}
