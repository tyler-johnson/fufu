//! `ff done` ends the editing session `ff edit` opened: the commit the
//! session was opened on is amended with what the working tree now holds,
//! what waited ahead is replayed onto it, and the worktree lands back on the
//! branch the session left standing. `--abandon` drops the session instead,
//! stashing whatever is uncommitted rather than discarding it. It is one
//! operation — the amend, the replay and the return move together — so one
//! `ff undo` takes the whole session back.

use ff_core::{DoneOutcome, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, abandon: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (outcome, verb_ctx) = ff_core::done::done(
        &repo,
        abandon,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    match outcome {
        DoneOutcome::Done(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "done": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("done", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            let short =
                crate::render::paint_sha(ff_core::sha::short(report.editing.as_str()), colored);
            if report.amended.is_none() {
                // A session that emptied its commit did not leave it
                // "unamended", so this arm wins over the other two.
                println!(
                    "the session emptied {short} \"{}\"; the commit is gone",
                    report.subject
                );
            } else if report.unchanged {
                println!(
                    "the session changed nothing; {short} \"{}\" is unamended",
                    report.subject
                );
            } else {
                println!("amended {short} \"{}\"", report.subject);
            }
            if report.replayed > 0 {
                if report.moved.is_empty() {
                    println!("replayed {} commit(s)", report.replayed);
                } else {
                    println!(
                        "replayed {} commit(s); moved {}",
                        report.replayed,
                        report.moved.join(", ")
                    );
                }
            }
            if let Some(line) =
                crate::render::dropped_line(&report.dropped, Some(&report.editing), colored)
            {
                println!("{line}");
            }
            println!("back on {}", report.onto);
            crate::cmd::switch::render_arrival(&report.arrival, colored);
            if report.published > 0 {
                // Disclosure, not a warning, on the same rule as restack — and
                // read off the report rather than asked of HEAD, because HEAD
                // was on the anonymous session branch, which has no upstream.
                let upstream_name = report.published_on.as_deref().unwrap_or("the remote");
                println!(
                    "{} of the rewritten commits are already on {}",
                    report.published, upstream_name
                );
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        DoneOutcome::Held(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "done": serde_json::Value::Null,
                    "held": report,
                });
                crate::machine::emit("done", &payload)?;
                crate::exit::held();
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            println!("{}", crate::render::held_block(&report, colored));
            crate::exit::held();
        }
        DoneOutcome::Abandoned(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "done": report,
                    "undo": "ff undo",
                });
                crate::machine::emit("done", &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            if report.editing.is_empty() {
                // A resolution, not an editing session: there is no commit
                // being edited to name, and saying so with an empty sha and
                // an empty subject is worse than saying what happened. You
                // never left the branch either, so there is nothing to say
                // about coming back to it.
                println!("abandoned the resolution on {}", report.onto);
                println!(
                    "{}",
                    crate::render::paint_dim("your open change is open again", colored)
                );
            } else {
                println!(
                    "abandoned the session on {} \"{}\"",
                    crate::render::paint_sha(ff_core::sha::short(report.editing.as_str()), colored),
                    report.subject
                );
                if let Some(stash) = &report.stashed {
                    // Nothing was lost: say where it went.
                    println!(
                        "stashed the session's edits ({})",
                        crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
                    );
                }
                println!("back on {}", report.onto);
                crate::cmd::switch::render_arrival(&report.arrival, colored);
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        DoneOutcome::Resolved(report) => {
            if ctx.json {
                let mut payload = serde_json::json!({
                    "done": report,
                    "undo": "ff undo",
                });
                if let Some(held) = &report.still_held {
                    payload["held"] = serde_json::json!(held);
                }
                crate::machine::emit("done", &payload)?;
                if report.still_held.is_some() {
                    crate::exit::held();
                }
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            let fixed = report.fixed;
            println!(
                "resolved {} conflict{}; replayed {} commit{}",
                fixed,
                if fixed == 1 { "" } else { "s" },
                report.replayed,
                if report.replayed == 1 { "" } else { "s" }
            );
            println!(
                "{} is now at {}",
                report.branch,
                crate::render::paint_sha(ff_core::sha::short(report.new_tip.as_str()), colored)
            );
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
            if let Some(held) = &report.still_held {
                // A rewrite is still waiting: render the hold and say so to
                // the shell, the way a plain hold does.
                println!("{}", crate::render::held_block(held, colored));
                crate::exit::held();
            }
        }
    }
    Ok(())
}
